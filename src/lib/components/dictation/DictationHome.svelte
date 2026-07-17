<script lang="ts">
    import { onMount } from "svelte";
    import { Copy, Mic, Speech, Square, Trash2 } from "lucide-svelte";
    import { dictationStore } from "$lib/stores/dictation.svelte";
    import { configStore } from "$lib/stores/config.svelte";
    import { appState } from "$lib/stores/app-state.svelte";
    import { Button } from "$lib/components/ui/button";
    import Tip from "$lib/components/Tip.svelte";
    import { formatCombo } from "$lib/utils/hotkey";
    import { cn } from "$lib/utils";

    onMount(() => {
        void dictationStore.refresh();
    });

    // Which rows show the raw transcript instead of the cleaned text.
    let showRaw = $state<Record<number, boolean>>({});
    let copiedId = $state<number | null>(null);

    let hotkey = $derived(configStore.config?.dictation_hotkey ?? "");

    function textOf(d: { id: number; raw_text: string; cleaned_text: string | null }): string {
        return showRaw[d.id] || !d.cleaned_text ? d.raw_text : d.cleaned_text;
    }

    async function copy(d: { id: number; raw_text: string; cleaned_text: string | null }) {
        await navigator.clipboard.writeText(textOf(d));
        copiedId = d.id;
        setTimeout(() => (copiedId = null), 1200);
    }

    function formatWhen(iso: string): string {
        const date = new Date(iso);
        return date.toLocaleString(undefined, {
            month: "short",
            day: "numeric",
            hour: "numeric",
            minute: "2-digit",
        });
    }
</script>

<div class="flex min-h-0 flex-1 flex-col">
    <div class="flex shrink-0 items-center justify-between border-b border-border px-4 py-3">
        <h2 class="font-display text-base tracking-tight">Dictation</h2>

        <!-- The hotkey is shown, not set: clicking it lands on the dictation
             settings page where the capture control lives. -->
        <div class="flex shrink-0 items-center gap-2">
            <Tip text="Change in dictation settings">
                {#snippet children({ props })}
                    <button
                        {...props}
                        class="min-w-28 rounded-md border border-input px-3 py-1.5 font-mono text-xs text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
                        aria-label="Dictation hotkey — change in settings"
                        onclick={() => appState.openSettings("dictation")}
                    >
                        {formatCombo(hotkey) || "Set a hotkey"}
                    </button>
                {/snippet}
            </Tip>
            <Button
                size="sm"
                variant={dictationStore.active ? "destructive" : "default"}
                onclick={() =>
                    dictationStore.active ? dictationStore.stop() : dictationStore.start()}
            >
                {#if dictationStore.active}
                    <Square size={13} class="mr-1" /> Stop
                {:else}
                    <Mic size={13} class="mr-1" /> Dictate
                {/if}
            </Button>
        </div>
    </div>

    {#if dictationStore.error}
        <p class="shrink-0 border-b border-border bg-destructive/5 px-4 py-2 text-xs text-destructive">
            {dictationStore.error}
        </p>
    {/if}

    <div class="min-h-0 flex-1 overflow-y-auto p-3">
        {#if dictationStore.history.length === 0}
            <div class="flex h-full flex-col items-center justify-center gap-3 text-center">
                <Speech size={26} class="text-muted-foreground/60" />
                <div class="max-w-sm space-y-1">
                    <p class="text-sm font-medium">Speak instead of typing</p>
                    <p class="text-xs leading-relaxed text-muted-foreground">
                        Dictations are transcribed on this computer, optionally cleaned
                        up by AI, pasted where your cursor is, and kept here.
                    </p>
                </div>
                <button
                    class="text-xs text-primary underline-offset-2 hover:underline"
                    onclick={() => appState.openSettings("dictation")}
                >
                    Open dictation settings
                </button>
            </div>
        {:else}
            <div class="mx-auto max-w-2xl space-y-2">
                {#each dictationStore.history as d (d.id)}
                    <div class="group rounded-lg border border-border p-3">
                        <div class="flex items-center gap-2 text-[11px] text-muted-foreground">
                            <span>{formatWhen(d.created_at)}</span>
                            {#if d.app}
                                <span class="rounded bg-muted px-1.5 py-0.5">{d.app}</span>
                            {/if}
                            {#if d.cleaned_text}
                                <button
                                    class={cn(
                                        "rounded px-1.5 py-0.5 transition-colors",
                                        showRaw[d.id]
                                            ? "bg-muted"
                                            : "bg-primary/10 text-primary",
                                    )}
                                    onclick={() => (showRaw[d.id] = !showRaw[d.id])}
                                >
                                    {showRaw[d.id] ? "raw" : "cleaned"}
                                </button>
                            {/if}
                            <span class="flex-1"></span>
                            <div
                                class="flex items-center gap-0.5 opacity-0 transition-opacity group-hover:opacity-100"
                            >
                                <Tip text="Copy">
                                    {#snippet children({ props })}
                                        <button
                                            {...props}
                                            class="rounded p-1 text-muted-foreground hover:bg-accent hover:text-foreground"
                                            aria-label="Copy"
                                            onclick={() => copy(d)}
                                        >
                                            <Copy size={12} />
                                        </button>
                                    {/snippet}
                                </Tip>
                                <Tip text="Delete">
                                    {#snippet children({ props })}
                                        <button
                                            {...props}
                                            class="rounded p-1 text-muted-foreground hover:bg-destructive/10 hover:text-destructive"
                                            aria-label="Delete"
                                            onclick={() => dictationStore.remove(d.id)}
                                        >
                                            <Trash2 size={12} />
                                        </button>
                                    {/snippet}
                                </Tip>
                            </div>
                        </div>
                        <p class="mt-1.5 text-sm leading-relaxed">
                            {textOf(d)}
                        </p>
                        {#if copiedId === d.id}
                            <p class="mt-1 text-[10px] text-primary">Copied</p>
                        {/if}
                    </div>
                {/each}
            </div>
        {/if}
    </div>
</div>
