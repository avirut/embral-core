<script lang="ts">
    // A small exclusive switch in the tabs-list visual language (the
    // bg-muted pill container), for the onboarding rows: English ↔
    // Multilingual, Fast / Balanced / Accurate. The recommended option
    // carries a small dot.
    import { cn } from "$lib/utils";
    import { copy } from "$lib/copy";

    const t = $derived(copy.onboarding.segmented);

    let {
        options,
        value,
        recommended,
        onSelect,
    }: {
        options: { value: string; label: string }[];
        value: string;
        /** The machine/system recommendation, marked with a dot. */
        recommended?: string;
        onSelect: (value: string) => void;
    } = $props();
</script>

<div
    class="inline-flex w-fit items-center justify-center rounded-lg bg-muted p-[3px] text-muted-foreground"
>
    {#each options as o (o.value)}
        <button
            type="button"
            class={cn(
                "inline-flex h-7 items-center gap-1.5 rounded-md px-3 text-xs font-medium transition-colors",
                o.value === value
                    ? "bg-background text-foreground shadow-sm"
                    : "hover:text-foreground",
            )}
            onclick={() => onSelect(o.value)}
        >
            {o.label}
            {#if o.value === recommended}
                <span
                    class="size-1 rounded-full bg-primary"
                    title={t.recommended}
                ></span>
            {/if}
        </button>
    {/each}
</div>
