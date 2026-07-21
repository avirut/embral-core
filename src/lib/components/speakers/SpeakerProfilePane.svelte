<script lang="ts">
    import { Trash2 } from "lucide-svelte";
    import type { SpeakerProfile } from "$lib/types";
    import { speakersStore } from "$lib/stores/speakers.svelte";
    import { Button } from "$lib/components/ui/button";
    import { Input } from "$lib/components/ui/input";
    import { Textarea } from "$lib/components/ui/textarea";
    import { Label } from "$lib/components/ui/label";

    let {
        speaker,
        onSaved,
        onDeleted,
    }: {
        speaker: SpeakerProfile | null;
        onSaved: (id: string) => void;
        onDeleted?: () => void;
    } = $props();

    let name = $state("");
    let notes = $state("");
    let loadedId = $state<string | null>(null);
    let confirmDelete = $state(false);

    // Load drafts whenever a different person is shown (null = new person).
    $effect(() => {
        const key = speaker?.id ?? "__new__";
        if (loadedId !== key) {
            loadedId = key;
            name = speaker?.name ?? "";
            notes = speaker?.notes ?? "";
            confirmDelete = false;
        }
    });

    let dirty = $derived(
        speaker === null
            ? name.trim().length > 0
            : name !== speaker.name || notes !== speaker.notes,
    );

    async function save() {
        const saved = await speakersStore.save({
            id: speaker?.id,
            name: name.trim(),
            notes,
        });
        if (saved) onSaved(saved.id);
    }

    async function remove() {
        if (!speaker) return;
        if (!confirmDelete) {
            confirmDelete = true;
            return;
        }
        await speakersStore.remove(speaker.id);
        onDeleted?.();
    }
</script>

<div class="mx-auto max-w-xl p-6">
    <h2 class="font-display text-lg tracking-tight">
        {speaker ? speaker.name : "New profile"}
    </h2>

    <div class="mt-5 space-y-4">
        <div class="space-y-1.5">
            <Label for="sp-name">Name</Label>
            <Input id="sp-name" bind:value={name} placeholder="Alice Johnson" />
        </div>
        <div class="space-y-1.5">
            <Label for="sp-notes">Notes</Label>
            <Textarea
                id="sp-notes"
                bind:value={notes}
                rows={3}
                placeholder="Anything worth remembering about this person"
            />
        </div>

        <div class="flex items-center gap-2">
            <Button size="sm" onclick={save} disabled={!dirty || !name.trim()}>
                {speaker ? "Save changes" : "Create profile"}
            </Button>
            {#if speakersStore.error}
                <p class="text-xs text-destructive">{speakersStore.error}</p>
            {/if}
        </div>
    </div>

    {#if speaker}
        <div class="mt-8 border-t border-border pt-4">
            <Button size="sm" variant="ghost" class="text-destructive" onclick={remove}>
                <Trash2 size={14} class="mr-1" />
                {confirmDelete ? "Really delete?" : `Delete ${speaker.name}`}
            </Button>
            <p class="mt-1 text-xs text-muted-foreground">
                Past transcripts keep the name as plain text.
            </p>
        </div>
    {/if}
</div>
