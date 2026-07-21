<script lang="ts">
    import { invoke } from "@tauri-apps/api/core";
    import {
        BookUser,
        FileText,
        FileUp,
        Mic,
        NotebookPen,
        Settings,
        Square,
        Type,
        UserPlus,
    } from "lucide-svelte";
    import { importRecording } from "$lib/utils/importRecording";
    import type { LibrarySearchResults } from "$lib/types";
    import { appState, type AppView } from "$lib/stores/app-state.svelte";
    import { meetingsStore } from "$lib/stores/meetings.svelte";
    import { configStore } from "$lib/stores/config.svelte";
    import { dictationStore } from "$lib/stores/dictation.svelte";
    import * as Command from "$lib/components/ui/command";

    let { open = $bindable(false) }: { open?: boolean } = $props();

    const noResults: LibrarySearchResults = { meetings: [], dictations: [] };

    let query = $state("");
    let results = $state<LibrarySearchResults>(noResults);
    let searching = $state(false);
    let searchTimer: ReturnType<typeof setTimeout> | null = null;
    /** Which search is the current one. A slower earlier query must not land on
     * top of a newer one's results — the palette would show answers to a question
     * the user has already moved on from. */
    let generation = 0;

    // Debounced hybrid query (meetings + dictations in one call); results
    // are already ranked by the backend, so the Command list renders them
    // as-is (shouldFilter=false — all filtering here is manual).
    $effect(() => {
        const q = query.trim();
        if (searchTimer) clearTimeout(searchTimer);
        if (!open || q.length < 2) {
            results = noResults;
            searching = false;
            generation++;
            return;
        }
        searching = true;
        searchTimer = setTimeout(async () => {
            const mine = ++generation;
            try {
                const found = await invoke<LibrarySearchResults>("search_library", {
                    query: q,
                    limit: 12,
                });
                if (mine !== generation) return;
                results = found;
            } catch {
                if (mine !== generation) return;
                results = noResults;
            } finally {
                if (mine === generation) searching = false;
            }
        }, 150);
    });

    $effect(() => {
        if (!open) {
            query = "";
            results = noResults;
        }
    });

    /// Split an FTS snippet on its [match] markers for highlighting.
    function snippetParts(
        snippet: string,
    ): { text: string; match: boolean }[] {
        const parts: { text: string; match: boolean }[] = [];
        const re = /\[([^\]]*)\]/g;
        let last = 0;
        for (const m of snippet.matchAll(re)) {
            if (m.index > last) {
                parts.push({ text: snippet.slice(last, m.index), match: false });
            }
            parts.push({ text: m[1], match: true });
            last = m.index + m[0].length;
        }
        if (last < snippet.length) {
            parts.push({ text: snippet.slice(last), match: false });
        }
        return parts;
    }

    function formatDate(iso: string): string {
        return new Date(iso).toLocaleDateString(undefined, {
            month: "short",
            day: "numeric",
            year: "numeric",
        });
    }

    function close(fn: () => void) {
        open = false;
        fn();
    }

    async function openMeeting(id: string) {
        open = false;
        appState.setView("idle");
        await meetingsStore.select(id);
    }

    async function startRecording() {
        open = false;
        try {
            await invoke("start_recording");
        } catch (e) {
            appState.setError(e instanceof Error ? e.message : String(e));
        }
    }

    async function stopRecording() {
        open = false;
        try {
            await invoke("stop_recording", { userNotes: null, meetingTitle: null });
        } catch (e) {
            appState.setError(e instanceof Error ? e.message : String(e));
        }
    }

    // Navigation entries, filtered manually against the query. The main
    // pages always show; settings pages surface as you type.
    const pages: { label: string; icon: typeof Mic; go: () => void }[] = [
        { label: "Meetings", icon: NotebookPen, go: () => appState.setView("idle") },
        { label: "Profiles", icon: BookUser, go: () => appState.setView("speakers") },
        { label: "Dictation", icon: Type, go: () => appState.setView("dictation") },
    ];
    // Ids follow SettingsLayout's SectionId values.
    const settingsPages: { id: string; label: string }[] = [
        { id: "general", label: "General" },
        { id: "meetings", label: "Meetings" },
        { id: "dictation", label: "Dictation" },
        { id: "about", label: "About" },
        { id: "markdown", label: "Markdown" },
        { id: "mcp", label: "MCP" },
        { id: "transcription", label: "Transcription" },
        { id: "synthesis", label: "Synthesis" },
    ];
    let settingsMatches = $derived.by(() => {
        const q = query.trim().toLowerCase();
        if (q.length < 2) return [];
        return settingsPages.filter(
            (p) =>
                p.label.toLowerCase().includes(q) ||
                "settings".includes(q) ||
                `settings ${p.label}`.toLowerCase().includes(q),
        );
    });
    let pageMatches = $derived.by(() => {
        const q = query.trim().toLowerCase();
        if (!q) return pages;
        return pages.filter((p) => p.label.toLowerCase().includes(q));
    });
</script>

<Command.Dialog
    bind:open
    shouldFilter={false}
    title="Search"
    description="Search meetings, dictations, and commands"
    class="sm:max-w-2xl"
>
    <Command.Input
        placeholder="Search meetings, dictations, and commands…"
        bind:value={query}
    />
    <Command.List>
        <!-- "No results" is only true once the search has actually finished.
             Saying it while a query is in flight tells the user their meeting
             isn't there, a moment before it appears. -->
        {#if query.trim().length >= 2 && !searching && results.meetings.length === 0 && results.dictations.length === 0}
            <Command.Empty>No results.</Command.Empty>
        {/if}

        {#if searching && results.meetings.length === 0}
            <Command.Loading>
                <p class="px-2 py-3 text-center text-sm text-muted-foreground">Searching…</p>
            </Command.Loading>
        {/if}

        {#if results.meetings.length > 0}
            <Command.Group heading="Meetings">
                {#each results.meetings as hit (hit.id)}
                    <Command.Item
                        value={hit.id}
                        onSelect={() => openMeeting(hit.id)}
                        class="flex-col items-start gap-0.5"
                    >
                        <div class="flex w-full items-center gap-2">
                            <FileText size={14} class="shrink-0 text-muted-foreground" />
                            <span class="truncate font-medium">{hit.title}</span>
                            <span class="ml-auto shrink-0 text-xs text-muted-foreground"
                                >{formatDate(hit.started_at)}</span
                            >
                        </div>
                        <p class="line-clamp-1 pl-6 text-xs text-muted-foreground">
                            {#each snippetParts(hit.snippet) as part, i (i)}
                                {#if part.match}<mark
                                        class="rounded-sm bg-primary/15 px-0.5 text-foreground"
                                        >{part.text}</mark
                                    >{:else}{part.text}{/if}
                            {/each}
                        </p>
                    </Command.Item>
                {/each}
            </Command.Group>
        {/if}

        {#if results.dictations.length > 0}
            <Command.Group heading="Dictations">
                {#each results.dictations as d (d.id)}
                    <Command.Item
                        value={`dictation-${d.id}`}
                        onSelect={() => close(() => appState.setView("dictation"))}
                        class="flex-col items-start gap-0.5"
                    >
                        <div class="flex w-full items-center gap-2">
                            <Type size={14} class="shrink-0 text-muted-foreground" />
                            <span class="line-clamp-1 min-w-0 text-sm">
                                {#each snippetParts(d.snippet) as part, i (i)}
                                    {#if part.match}<mark
                                            class="rounded-sm bg-primary/15 px-0.5 text-foreground"
                                            >{part.text}</mark
                                        >{:else}{part.text}{/if}
                                {/each}
                            </span>
                            <span class="ml-auto shrink-0 text-xs text-muted-foreground"
                                >{formatDate(d.created_at)}</span
                            >
                        </div>
                    </Command.Item>
                {/each}
            </Command.Group>
        {/if}

        <Command.Group heading="Actions">
            {#if appState.isRecording}
                <Command.Item value="action-stop" onSelect={stopRecording}>
                    <Square size={14} />
                    Stop recording
                </Command.Item>
            {:else if configStore.isConfigured}
                <Command.Item value="action-record" onSelect={startRecording}>
                    <Mic size={14} />
                    Start recording
                </Command.Item>
                <Command.Item
                    value="action-dictate"
                    onSelect={() => close(() => void dictationStore.start())}
                >
                    <Type size={14} />
                    Start dictation
                </Command.Item>
            {/if}
            <Command.Item
                value="action-import"
                onSelect={() => close(() => void importRecording())}
            >
                <FileUp size={14} />
                Import a recording…
            </Command.Item>
            <Command.Item
                value="action-new-profile"
                onSelect={() => close(() => appState.openProfilesCreate())}
            >
                <UserPlus size={14} />
                New profile
            </Command.Item>
        </Command.Group>

        <Command.Group heading="Go to">
            {#each pageMatches as p (p.label)}
                <Command.Item
                    value={`nav-${p.label}`}
                    onSelect={() => close(p.go)}
                >
                    <p.icon size={14} />
                    {p.label}
                </Command.Item>
            {/each}
            {#if settingsMatches.length === 0}
                <Command.Item
                    value="nav-settings"
                    onSelect={() => close(() => appState.openSettings())}
                >
                    <Settings size={14} />
                    Settings
                </Command.Item>
            {/if}
            {#each settingsMatches as s (s.id)}
                <Command.Item
                    value={`nav-settings-${s.id}`}
                    onSelect={() => close(() => appState.openSettings(s.id))}
                >
                    <Settings size={14} />
                    Settings → {s.label}
                </Command.Item>
            {/each}
        </Command.Group>
    </Command.List>
</Command.Dialog>
