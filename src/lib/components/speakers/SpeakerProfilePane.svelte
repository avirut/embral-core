<script lang="ts">
    import { Mic, Square, Trash2 } from "lucide-svelte";
    import type { ModelStatus, SpeakerProfile } from "$lib/types";
    import { speakersStore } from "$lib/stores/speakers.svelte";
    import { appState } from "$lib/stores/app-state.svelte";
    import AudioPlayer from "$lib/components/AudioPlayer.svelte";
    import { Button } from "$lib/components/ui/button";
    import { Input } from "$lib/components/ui/input";
    import { Textarea } from "$lib/components/ui/textarea";
    import { Switch } from "$lib/components/ui/switch";
    import { Label } from "$lib/components/ui/label";

    let {
        speaker,
        speakerIdModel,
        onSaved,
        onDeleted,
    }: {
        speaker: SpeakerProfile | null;
        speakerIdModel: ModelStatus | undefined;
        onSaved: (id: string) => void;
        onDeleted?: () => void;
    } = $props();

    let name = $state("");
    let notes = $state("");
    let isYou = $state(false);
    let loadedId = $state<string | null>(null);
    let confirmDelete = $state(false);
    let countdown = $state(0);
    let countdownTimer: ReturnType<typeof setInterval> | null = null;

    const ENROLL_SECS = 10;

    // Load drafts whenever a different person is shown (null = new person).
    $effect(() => {
        const key = speaker?.id ?? "__new__";
        if (loadedId !== key) {
            loadedId = key;
            name = speaker?.name ?? "";
            notes = speaker?.notes ?? "";
            isYou = speaker?.is_you ?? false;
            confirmDelete = false;
        }
    });

    let dirty = $derived(
        speaker === null
            ? name.trim().length > 0
            : name !== speaker.name ||
                  notes !== speaker.notes ||
                  isYou !== speaker.is_you,
    );

    async function save() {
        const saved = await speakersStore.save({
            id: speaker?.id,
            name: name.trim(),
            notes,
            is_you: isYou,
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

    function slotKey(slot: number) {
        return speaker ? `${speaker.id}:${slot}` : "";
    }

    async function recordSlot(slot: number) {
        if (!speaker) return;
        countdown = ENROLL_SECS;
        countdownTimer = setInterval(() => {
            countdown = Math.max(0, countdown - 1);
        }, 1000);
        try {
            await speakersStore.recordSlot(speaker.id, slot);
        } finally {
            if (countdownTimer) clearInterval(countdownTimer);
            countdownTimer = null;
            countdown = 0;
        }
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
        <div class="flex items-center justify-between rounded-lg border border-border p-3">
            <div>
                <p class="text-sm font-medium">This is me</p>
                <p class="text-xs text-muted-foreground">
                    Your own microphone speech gets attributed to this person.
                </p>
            </div>
            <Switch bind:checked={isYou} />
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
        <div class="mt-8">
            <h3 class="text-sm font-semibold">Voice references</h3>
            <p class="mt-1 text-xs text-muted-foreground">
                Short clips of {speaker.name} speaking, used to recognize their
                voice in future meetings. Record up to three in different
                settings for best results.
            </p>
            {#if speakerIdModel && !speakerIdModel.present}
                <button
                    class="mt-2 text-xs text-primary underline-offset-2 hover:underline"
                    onclick={() => appState.openSettings("transcription")}
                >
                    Recording references needs the speaker identification models —
                    download them in Settings.
                </button>
            {/if}
            <div class="mt-3 space-y-2">
                {#each speaker.voice_slots as slot (slot.slot)}
                    {@const recording = speakersStore.recordingSlot === slotKey(slot.slot)}
                    <div class="flex items-center gap-3 rounded-lg border border-border p-3">
                        <span class="w-12 shrink-0 text-xs text-muted-foreground">
                            Clip {slot.slot}
                        </span>
                        <div class="min-w-0 flex-1">
                            {#if recording}
                                <div class="flex items-center gap-2">
                                    <span class="h-2 w-2 animate-pulse rounded-full bg-destructive"></span>
                                    <span class="text-sm">
                                        Speak normally… {countdown}s
                                    </span>
                                </div>
                            {:else if slot.clip_path}
                                <AudioPlayer audioPath={slot.clip_path} />
                            {:else}
                                <span class="text-xs text-muted-foreground">Empty</span>
                            {/if}
                        </div>
                        <div class="flex shrink-0 items-center gap-1">
                            {#if recording}
                                <Button
                                    size="sm"
                                    variant="outline"
                                    onclick={() => speakersStore.cancelRecording()}
                                >
                                    <Square size={13} class="mr-1" /> Stop
                                </Button>
                            {:else}
                                <Button
                                    size="sm"
                                    variant="outline"
                                    disabled={speakersStore.recordingSlot !== null ||
                                        (speakerIdModel && !speakerIdModel.present)}
                                    onclick={() => recordSlot(slot.slot)}
                                >
                                    <Mic size={13} class="mr-1" />
                                    {slot.ref_id != null ? "Re-record" : "Record"}
                                </Button>
                                {#if slot.ref_id != null}
                                    <Button
                                        size="sm"
                                        variant="ghost"
                                        aria-label="Delete clip"
                                        onclick={() => speakersStore.deleteSlot(slot.ref_id!)}
                                    >
                                        <Trash2 size={13} />
                                    </Button>
                                {/if}
                            {/if}
                        </div>
                    </div>
                {/each}
            </div>
            {#if speaker.learned_refs > 0}
                <p class="mt-2 text-xs text-muted-foreground">
                    Also learned from {speaker.learned_refs} confirmed
                    {speaker.learned_refs === 1 ? "meeting" : "meetings"}.
                </p>
            {/if}
        </div>

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
