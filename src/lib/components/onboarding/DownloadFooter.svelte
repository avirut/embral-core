<script lang="ts">
    // The wizard's persistent download line: aggregate progress across every
    // in-flight model, alive on whatever step the user is on (the store is
    // module-scoped, so downloads outlive the step that started them).
    import { modelsStore } from "$lib/stores/models.svelte";
    import { copy } from "$lib/copy";

    const t = $derived(copy.onboarding.download);

    let active = $derived(
        modelsStore.statuses.filter((m) => modelsStore.isDownloading(m.id)),
    );
    let pct = $derived.by(() => {
        let done = 0;
        let total = 0;
        for (const m of active) {
            const p = modelsStore.progress(m.id);
            if (p && p.total_bytes > 0) {
                done += p.downloaded_bytes;
                total += p.total_bytes;
            } else {
                total += m.total_bytes;
            }
        }
        return total > 0 ? Math.round((done / total) * 100) : 0;
    });
    let failed = $derived(
        modelsStore.statuses.filter(
            (m) => !m.present && modelsStore.error(m.id),
        ),
    );
</script>

{#if active.length > 0}
    <div class="space-y-1">
        <div class="h-1.5 w-full overflow-hidden rounded-full bg-muted">
            <div
                class="h-full bg-primary transition-all duration-300"
                style="width: {pct}%"
            ></div>
        </div>
        <p class="text-xs text-muted-foreground">
            {t.active(active.length, pct)}
        </p>
    </div>
{:else if failed.length > 0}
    <p class="text-xs text-destructive">
        {t.failed(failed[0].display_name)}
        <button
            type="button"
            class="underline underline-offset-2"
            onclick={() => void modelsStore.download(failed[0].id)}
        >
            {t.retry}
        </button>
    </p>
{/if}
