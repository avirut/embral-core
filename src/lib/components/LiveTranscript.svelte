<script lang="ts">
    import { onMount } from "svelte";
    import { invoke } from "@tauri-apps/api/core";
    import { ArrowDown, Star } from "lucide-svelte";
    import { appState } from "$lib/stores/app-state.svelte";
    import { speakersStore } from "$lib/stores/speakers.svelte";
    import { formatTime } from "$lib/utils/meetingFormat";
    import { chipClass } from "$lib/utils/speakerColors";
    import SpeakerNameInput from "./SpeakerNameInput.svelte";
    import Tip from "./Tip.svelte";
    import type { TranscriptionSegment } from "$lib/types";

    interface Group {
        speaker: string | null;
        texts: string[];
        start: number;
    }

    // Paragraph segmentation: keep in sync with the Rust counterpart in
    // src-tauri/src/commands.rs (`starts_new_paragraph`).
    const STRONG_GAP = 4.0;
    const SOFT_GAP = 2.0;
    const MAX_PARAGRAPH_CHARS = 800;
    const SENTENCE_END = /[.!?]$/;

    function startsNewParagraph(
        prev: TranscriptionSegment,
        curr: TranscriptionSegment,
        runningLen: number,
    ): boolean {
        if (prev.speaker !== curr.speaker) return true;
        const gap = curr.start - prev.end;
        if (gap >= STRONG_GAP) return true;
        if (gap >= SOFT_GAP && SENTENCE_END.test(prev.text.trimEnd()))
            return true;
        if (runningLen + curr.text.length + 1 > MAX_PARAGRAPH_CHARS)
            return true;
        return false;
    }

    let starSeconds = $derived(
        appState.stars.map((s) => s.seconds).sort((a, b) => a - b),
    );

    let grouped = $derived.by(() => {
        // Segments arrive in monotonic time order, so we group in arrival
        // order without re-sorting (keep in sync with format_transcript).
        const segs = appState.segments;
        if (!segs.length) return [] as Group[];

        // A star between two segments is a hard paragraph break: the
        // session split the utterance at the star, and joining the halves
        // back into one group would pull the post-star words above the
        // marker (a group carries its FIRST segment's start time).
        const starBetween = (prev: TranscriptionSegment, curr: TranscriptionSegment) =>
            starSeconds.some((s) => s > prev.start && s <= curr.start);

        const groups: Group[] = [];
        let current: Group = {
            speaker: segs[0].speaker,
            texts: [segs[0].text],
            start: segs[0].start,
        };
        let runningLen = segs[0].text.length;

        for (let i = 1; i < segs.length; i++) {
            if (
                startsNewParagraph(segs[i - 1], segs[i], runningLen) ||
                starBetween(segs[i - 1], segs[i])
            ) {
                groups.push(current);
                current = {
                    speaker: segs[i].speaker,
                    texts: [segs[i].text],
                    start: segs[i].start,
                };
                runningLen = segs[i].text.length;
            } else {
                current.texts.push(segs[i].text);
                runningLen += segs[i].text.length + 1; // +1 for the join-space
            }
        }
        groups.push(current);
        return groups;
    });

    // Paragraph groups and star markers merged into one time-ordered stream.
    type StreamItem =
        | { kind: "group"; group: Group }
        | { kind: "star"; secs: number };
    let stream = $derived.by(() => {
        const items: StreamItem[] = [];
        const stars = starSeconds;
        let si = 0;
        for (const group of grouped) {
            while (si < stars.length && stars[si] <= group.start) {
                items.push({ kind: "star", secs: stars[si++] });
            }
            items.push({ kind: "group", group });
        }
        while (si < stars.length) {
            items.push({ kind: "star", secs: stars[si++] });
        }
        return items;
    });

    // Speaker labels in first-appearance order — drives chip colors and the
    // pills row (rendered only when live labels exist at all).
    let labels = $derived.by(() => {
        const seen: string[] = [];
        for (const s of appState.segments) {
            if (s.speaker && !seen.includes(s.speaker)) seen.push(s.speaker);
        }
        return seen;
    });

    onMount(() => {
        if (!speakersStore.loaded) void speakersStore.refresh();
    });

    // --- Renaming a live speaker (double-click a pill). Typing a profile's
    // name links it; a new name creates the profile on the spot. The rename
    // reaches the backend accumulator and future segments of that cluster
    // (`rename_live_speaker`), and the post-meeting pass keeps user-given
    // names for the clusters they cover.
    let editingLabel = $state<string | null>(null);
    let labelDraft = $state("");
    let renameBusy = $state(false);

    function startLabelEdit(label: string) {
        if (renameBusy) return;
        editingLabel = label;
        labelDraft = label;
    }

    async function commitLabelEdit() {
        const from = editingLabel;
        const to = labelDraft.trim();
        editingLabel = null;
        if (!from || !to || to === from || renameBusy) return;
        renameBusy = true;
        try {
            // A real name that isn't in the registry yet becomes a profile
            // on the spot; merging into a generic "Speaker N" never does.
            const known = speakersStore.speakers.some(
                (p) => p.name.toLowerCase() === to.toLowerCase(),
            );
            if (!known && !/^Speaker \d+$/.test(to)) {
                await speakersStore.save({
                    name: to,
                    notes: "",
                });
            }
            await invoke("rename_live_speaker", { from, to });
            appState.renameSpeaker(from, to);
        } catch (e) {
            console.error("live speaker rename failed:", e);
        } finally {
            renameBusy = false;
        }
    }

    // Word lists for the interim block: index-keyed spans mean words already
    // on screen keep their DOM nodes (no re-animation) while appended words
    // mount with the fade-in.
    let interimStable = $derived(
        appState.interim?.text ? appState.interim.text.split(/\s+/) : [],
    );
    let interimTentative = $derived.by(() => {
        const tail = appState.interim?.tentative_text?.trim();
        return tail ? tail.split(/\s+/) : [];
    });
    // The raw tail's leading space (or its absence) is the word boundary
    // (see the interim contract in types.ts): a spaceless tail continues
    // the last stable word — "keep tal" + "king" — so that word must not
    // render its trailing space.
    let tentativeJoinsWord = $derived.by(() => {
        const tail = appState.interim?.tentative_text;
        return !!tail && !/^\s/.test(tail) && interimStable.length > 0;
    });

    // Pinned-to-bottom auto-scroll: scrolling up unpins (new content stops
    // yanking the viewport); the pill or reaching the bottom re-pins.
    let scrollEl = $state<HTMLElement | null>(null);
    let pinned = $state(true);

    function onScroll() {
        if (!scrollEl) return;
        pinned =
            scrollEl.scrollTop + scrollEl.clientHeight >=
            scrollEl.scrollHeight - 40;
    }

    function jumpToLatest() {
        pinned = true;
        scrollEl?.scrollTo({ top: scrollEl.scrollHeight, behavior: "smooth" });
    }

    $effect(() => {
        // Re-run when segments, the interim preview, or the stars change.
        appState.segments;
        appState.interim;
        appState.stars;
        if (scrollEl && pinned) {
            scrollEl.scrollTop = scrollEl.scrollHeight;
        }
    });
</script>

<div class="flex h-full min-h-0 flex-col">
    {#if labels.length > 0}
        <div class="shrink-0 border-b border-border px-4 py-2">
            <div class="flex flex-wrap items-center gap-1.5">
                <span
                    class="mr-0.5 text-[11px] font-medium tracking-wide text-muted-foreground uppercase"
                >
                    Speakers
                </span>
                {#each labels as label (label)}
                    {#if editingLabel === label}
                        <SpeakerNameInput
                            bind:value={labelDraft}
                            suggestions={[
                                ...speakersStore.speakers.map((p) => p.name),
                                ...labels.filter((l) => l !== label),
                            ]}
                            onCommit={commitLabelEdit}
                            onCancel={() => (editingLabel = null)}
                        />
                    {:else}
                        <Tip text="Name this speaker">
                            {#snippet children({ props })}
                                <button
                                    {...props}
                                    class="rounded-full px-2 py-0.5 text-[11px] font-medium transition-opacity hover:opacity-75 {chipClass(
                                        label,
                                        labels,
                                    )}"
                                    onclick={() => startLabelEdit(label)}
                                >
                                    {label}
                                </button>
                            {/snippet}
                        </Tip>
                    {/if}
                {/each}
            </div>
        </div>
    {/if}

    <div class="relative min-h-0 flex-1">
        <div
            bind:this={scrollEl}
            onscroll={onScroll}
            class="no-scrollbar h-full space-y-3 overflow-y-auto px-4 py-3"
        >
            {#each stream as item, i (item.kind === "group"
                ? `g:${i}:${item.group.start}`
                : `s:${i}:${item.secs}`)}
                {#if item.kind === "star"}
                    <!-- A starred moment: a quiet rule at its timepoint. -->
                    <div
                        class="flex items-center gap-2 text-[11px] text-muted-foreground"
                    >
                        <Star size={11} fill="currentColor" class="shrink-0" />
                        <span class="tabular-nums">{formatTime(item.secs)}</span>
                        <div class="h-px min-w-0 flex-1 bg-border"></div>
                    </div>
                {:else}
                    <!-- No mount animation on finalized groups: their text was
                         (almost always) just on screen as the interim, and an
                         animation here reads as a flicker at the hand-off. -->
                    <div>
                        <div
                            class="flex items-baseline gap-2 text-[11px] text-muted-foreground"
                        >
                            <span class="tabular-nums">
                                {formatTime(item.group.start)}
                            </span>
                            {#if item.group.speaker}
                                <span
                                    class="rounded-full px-2 py-px text-[10px] font-medium {chipClass(
                                        item.group.speaker,
                                        labels,
                                    )}">{item.group.speaker}</span
                                >
                            {/if}
                        </div>
                        <p class="mt-0.5 text-[15px] leading-relaxed">
                            {item.group.texts.join(" ")}
                        </p>
                    </div>
                {/if}
            {/each}

            {#if appState.interim}
                <div class="group-rise">
                    <div
                        class="flex items-baseline gap-2 text-[11px] text-muted-foreground/70"
                    >
                        <span class="tabular-nums">
                            {formatTime(appState.interim.start)}
                        </span>
                    </div>
                    <!-- Whitespace-tight: any newline between these spans
                         renders as a space and would defeat the mid-word
                         join between the last stable word and the first
                         tentative one. Words carry their own trailing
                         space instead. -->
                    <p class="mt-0.5 text-[15px] leading-relaxed">
                        {#each interimStable as word, i (i)}<span
                                class="word-fade"
                                >{word}{i === interimStable.length - 1 &&
                                tentativeJoinsWord
                                    ? ""
                                    : " "}</span
                            >{/each}{#each interimTentative as word, i (i)}<span
                                class="word-fade text-muted-foreground/60"
                                >{word}{" "}</span
                            >{/each}<span class="interim-caret" aria-hidden="true"
                        ></span>
                    </p>
                </div>
            {/if}

            {#if grouped.length === 0 && !appState.interim}
                <p class="text-xs text-muted-foreground italic">Listening…</p>
            {/if}
        </div>

        {#if !pinned}
            <button
                onclick={jumpToLatest}
                class="absolute bottom-3 left-1/2 inline-flex -translate-x-1/2 items-center gap-1 rounded-full border border-border bg-background/95 px-3 py-1 text-[11px] font-medium text-muted-foreground shadow-sm transition-colors hover:text-foreground"
            >
                Jump to latest
                <ArrowDown size={11} />
            </button>
        {/if}
    </div>
</div>

<style>
    /* A live feed doesn't need a visible scrollbar — content length swings
       with the interim tail, and the thumb popping in and out reads as
       jitter. Wheel scrolling and the jump pill cover navigation. */
    .no-scrollbar {
        scrollbar-width: none;
    }
    .no-scrollbar::-webkit-scrollbar {
        display: none;
    }
    .group-rise {
        animation: rise 150ms ease-out;
    }
    @keyframes rise {
        from {
            opacity: 0;
            transform: translateY(6px);
        }
    }
    .word-fade {
        animation: word-in 120ms ease-out;
    }
    @keyframes word-in {
        from {
            opacity: 0;
        }
    }
    .interim-caret {
        display: inline-block;
        width: 2px;
        height: 1em;
        margin-left: 1px;
        vertical-align: text-bottom;
        background: var(--muted-foreground);
        animation: caret-pulse 1s ease-in-out infinite;
    }
    @keyframes caret-pulse {
        0%,
        100% {
            opacity: 0.7;
        }
        50% {
            opacity: 0.15;
        }
    }
</style>
