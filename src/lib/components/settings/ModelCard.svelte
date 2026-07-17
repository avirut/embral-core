<script lang="ts">
    // One model in the library, as a card: a colored left edge for status
    // (green ready, amber downloading, gray not downloaded), name, the use
    // case, the size, and the download/remove footer. Shared by the
    // Transcription and Synthesis pages so the library reads as one surface.
    import type { ModelStatus } from "$lib/types";
    import { modelsStore } from "$lib/stores/models.svelte";
    import { formatBytes } from "$lib/utils/bytes";
    import { Button } from "$lib/components/ui/button";

    let { model }: { model: ModelStatus } = $props();

    let downloading = $derived(modelsStore.isDownloading(model.id));
    let pct = $derived(Math.round((modelsStore.fraction(model.id) ?? 0) * 100));

    let edge = $derived(
        downloading
            ? "border-l-amber-500"
            : model.present
              ? "border-l-emerald-500"
              : "border-l-muted-foreground/40",
    );
</script>

<div
    class="flex flex-col rounded-lg border border-border border-l-2 {edge} bg-card p-4"
>
    <p class="text-sm font-medium">{model.display_name}</p>

    <p class="mt-1 text-xs leading-relaxed text-muted-foreground">{model.note}</p>

    <p class="mt-1.5 text-[11px] text-muted-foreground/70">
        {formatBytes(model.total_bytes)}
    </p>

    <div class="mt-auto pt-3">
        {#if downloading}
            <div class="space-y-1.5">
                <div class="h-1.5 w-full overflow-hidden rounded-full bg-muted">
                    <div
                        class="h-full bg-primary transition-all duration-300"
                        style="width: {pct}%"
                    ></div>
                </div>
                <p class="text-xs text-muted-foreground">Downloading… {pct}%</p>
            </div>
        {:else}
            <div class="flex gap-2">
                <Button size="sm" onclick={() => modelsStore.download(model.id)}>
                    {model.present ? "Re-download" : "Download"}
                </Button>
                {#if model.present}
                    <Button
                        variant="outline"
                        size="sm"
                        onclick={() => modelsStore.remove(model.id)}
                    >
                        Remove
                    </Button>
                {/if}
            </div>
        {/if}

        {#if modelsStore.error(model.id)}
            <p class="mt-2 text-xs text-destructive">{modelsStore.error(model.id)}</p>
        {/if}
    </div>
</div>
