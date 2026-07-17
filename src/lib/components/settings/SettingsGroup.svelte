<script lang="ts">
    import type { Snippet } from "svelte";

    let {
        label = "",
        children,
    }: { label?: string; children: Snippet } = $props();
</script>

<section class="space-y-2">
    {#if label}
        <h3 class="px-1 text-xs font-medium uppercase tracking-wider text-muted-foreground">
            {label}
        </h3>
    {/if}
    <div class="divide-y divide-border rounded-lg border border-border bg-card">
        {@render children()}
    </div>
</section>
