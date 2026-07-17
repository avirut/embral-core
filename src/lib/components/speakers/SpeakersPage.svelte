<script lang="ts">
    import { onMount } from "svelte";
    import { Plus, Trash2, Users } from "lucide-svelte";
    import { speakersStore } from "$lib/stores/speakers.svelte";
    import { modelsStore } from "$lib/stores/models.svelte";
    import { appState } from "$lib/stores/app-state.svelte";
    import SpeakerProfilePane from "./SpeakerProfilePane.svelte";
    import ResizableSplit from "$lib/components/ResizableSplit.svelte";
    import OverlayScroll from "$lib/components/OverlayScroll.svelte";
    import ConfirmDialog from "$lib/components/ConfirmDialog.svelte";
    import Tip from "$lib/components/Tip.svelte";
    import { Button } from "$lib/components/ui/button";
    import { groupByDate } from "$lib/utils/meetingFormat";
    import { ListSelection } from "$lib/utils/listSelection.svelte";
    import { cn } from "$lib/utils";

    let selectedId = $state<string | null>(null);
    let creating = $state(false);
    let confirmDelete = $state(false);
    let deleting = $state(false);

    // The same selection model as the meetings list — the two lists are one
    // object with different contents.
    const selection = new ListSelection();

    onMount(() => {
        void speakersStore.refresh();
        void modelsStore.refresh();
    });

    let speakers = $derived(speakersStore.speakers);

    // Grouped under the same date headers as the meetings list, by when each
    // person was last in a meeting — so the headings read "who you met with
    // today". Someone never seen in one falls back to when they were added,
    // which keeps a just-created profile at the top where the user is looking.
    // The backend already returns them in this order.
    let groups = $derived(
        groupByDate(speakers, (s) => s.last_seen ?? s.created_at),
    );
    let selected = $derived(
        selectedId ? (speakersStore.byId(selectedId) ?? null) : null,
    );
    let speakerIdModel = $derived(modelsStore.status("speaker-id"));

    let visibleOrder = $derived(groups.flatMap((g) => g.items.map((s) => s.id)));
    const multi = $derived(selection.count > 1);

    $effect(() => {
        if (!creating && !selected && speakers.length > 0) {
            selectedId = speakers[0].id;
            selection.select(speakers[0].id);
        }
    });

    function onRowClick(id: string, event: MouseEvent) {
        creating = false;
        selection.click(id, event, visibleOrder);
        selectedId = selection.primary;
    }

    function onKeydown(e: KeyboardEvent) {
        if (e.key !== "Delete" || confirmDelete || selection.count === 0) return;
        const target = e.target as HTMLElement | null;
        if (
            target &&
            (target.tagName === "INPUT" ||
                target.tagName === "TEXTAREA" ||
                target.isContentEditable)
        ) {
            return;
        }
        e.preventDefault();
        confirmDelete = true;
    }

    async function deleteSelected() {
        deleting = true;
        try {
            await speakersStore.removeMany(selection.ids);
            selection.clear();
            selectedId = null;
            confirmDelete = false;
        } finally {
            deleting = false;
        }
    }

    // Palette "New profile" lands here in create mode.
    $effect(() => {
        if (appState.profilesCreateRequest) {
            appState.clearProfilesCreateRequest();
            startCreate();
        }
    });

    function startCreate() {
        creating = true;
        selectedId = null;
    }

    function onSaved(id: string) {
        creating = false;
        selectedId = id;
    }
</script>

<div class="flex min-h-0 flex-1">
    <ResizableSplit
        fixedSide="left"
        storageKey="embral:profiles-list-width"
        defaultSize={280}
        minFixed={220}
        minFlex={420}
    >
        {#snippet left()}
            <!-- People list: no header, just the scrollable list with the
                 floating add button — the same object as the meetings pane,
                 down to the row treatment and the date headers. -->
            <div
                class="relative flex min-h-0 w-full flex-1 flex-col border-r border-border bg-muted/20"
            >
                <OverlayScroll>
                    <div class="pb-16">
                        {#each groups as group (group.label)}
                            <p
                                class="px-3 pt-3 pb-1 text-[11px] font-medium tracking-wide text-muted-foreground/80 uppercase"
                            >
                                {group.label}
                            </p>
                            {#each group.items as s (s.id)}
                                <button
                                    class={cn(
                                        "w-full border-l-2 px-3 py-2 text-left transition-colors duration-150",
                                        selectedId === s.id && !creating
                                            ? "border-l-foreground/60 bg-accent/50"
                                            : selection.has(s.id) && !creating
                                              ? "border-l-transparent bg-accent/40"
                                              : "border-l-transparent hover:bg-accent/40",
                                    )}
                                    onclick={(e) => onRowClick(s.id, e)}
                                >
                                    <div class="flex items-center justify-between gap-2">
                                        <span class="font-display min-w-0 truncate text-sm"
                                            >{s.name}</span
                                        >
                                        {#if s.is_you}
                                            <span
                                                class="shrink-0 rounded-full bg-primary/10 px-1.5 py-0.5 text-[10px] font-medium text-primary"
                                            >
                                                you
                                            </span>
                                        {/if}
                                    </div>
                                </button>
                            {/each}
                        {/each}
                        {#if speakers.length === 0 && speakersStore.loaded && !creating}
                            <p class="px-3 py-2 text-xs text-muted-foreground">
                                No profiles yet.
                            </p>
                        {/if}
                    </div>
                </OverlayScroll>

                <Tip side="left" text="Add a profile">
                    {#snippet children({ props })}
                        <button
                            {...props}
                            onclick={startCreate}
                            class="absolute right-3 bottom-3 flex h-10 w-10 items-center justify-center rounded-2xl bg-primary text-primary-foreground shadow-md transition-colors hover:bg-primary/90"
                            aria-label="Add a profile"
                        >
                            <Plus size={18} />
                        </button>
                    {/snippet}
                </Tip>
            </div>
        {/snippet}
        {#snippet right()}
            <!-- Profile / empty state -->
            <div class="min-h-0 min-w-0 flex-1 overflow-y-auto">
                {#if multi && !creating}
                    <!-- Several people picked: no single profile to show. -->
                    <div
                        class="flex h-full flex-col items-center justify-center gap-4 p-8"
                    >
                        <p class="text-sm text-muted-foreground">
                            {selection.count} profiles selected
                        </p>
                        <button
                            onclick={() => (confirmDelete = true)}
                            class="inline-flex h-9 items-center gap-2 rounded-md border border-border px-3 text-sm font-medium transition-colors hover:bg-destructive hover:text-white"
                        >
                            <Trash2 size={15} />
                            Delete {selection.count}
                        </button>
                        <p class="text-xs text-muted-foreground">Or press Delete.</p>
                    </div>
                {:else if creating}
                    <SpeakerProfilePane
                        speaker={null}
                        {speakerIdModel}
                        onSaved={onSaved}
                    />
                {:else if selected}
                    <SpeakerProfilePane
                        speaker={selected}
                        {speakerIdModel}
                        onSaved={onSaved}
                        onDeleted={() => (selectedId = null)}
                    />
                {:else}
                    <div
                        class="flex h-full flex-col items-center justify-center gap-3 p-8 text-center"
                    >
                        <Users size={28} class="text-muted-foreground/60" />
                        <div class="max-w-md space-y-1.5">
                            <p class="text-sm font-medium">Know who said what</p>
                            <p class="text-xs leading-relaxed text-muted-foreground">
                                Save the people you meet with. embral tells speakers
                                apart in every recording, and voice references let it
                                recognize the same people across meetings — by
                                suggestion or automatically, your choice in Settings.
                            </p>
                        </div>
                        <Button size="sm" onclick={startCreate}>
                            <Plus size={15} class="mr-1" /> Add a profile
                        </Button>
                        {#if speakerIdModel && !speakerIdModel.present}
                            <button
                                class="text-xs text-primary underline-offset-2 hover:underline"
                                onclick={() => appState.openSettings("transcription")}
                            >
                                Download the speaker identification models in Settings
                                first
                            </button>
                        {/if}
                    </div>
                {/if}
            </div>
        {/snippet}
    </ResizableSplit>
</div>

<svelte:window onkeydown={onKeydown} />

<ConfirmDialog
    bind:open={confirmDelete}
    title={selection.count === 1
        ? "Delete profile?"
        : `Delete ${selection.count} profiles?`}
    body="Their saved voice clips are deleted too. Transcripts keep the names already written into them."
    confirmLabel={selection.count === 1 ? "Delete" : `Delete ${selection.count}`}
    busy={deleting}
    onConfirm={deleteSelected}
/>
