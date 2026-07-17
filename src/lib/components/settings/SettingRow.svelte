<script lang="ts">
    import type { Snippet } from "svelte";

    let {
        title,
        description = "",
        vertical = false,
        titleExtra,
        descriptionExtra,
        children,
    }: {
        title: string;
        description?: string;
        /// Stack the control under the text (for wide controls like inputs).
        vertical?: boolean;
        /// Rendered inline after the title (e.g. a muted info button).
        titleExtra?: Snippet;
        /// Rendered where the description sits (e.g. a clickable subtext
        /// link); shown after the description when both are present.
        descriptionExtra?: Snippet;
        children?: Snippet;
    } = $props();
</script>

<div
    class={vertical
        ? "flex flex-col gap-2 px-4 py-3"
        : "flex items-center justify-between gap-6 px-4 py-3"}
>
    <div class="min-w-0">
        <p class="flex items-center gap-1.5 text-sm font-medium">
            {title}
            {#if titleExtra}{@render titleExtra()}{/if}
        </p>
        {#if description}
            <p class="mt-0.5 text-xs text-muted-foreground">{description}</p>
        {/if}
        {#if descriptionExtra}
            <p class="mt-0.5">{@render descriptionExtra()}</p>
        {/if}
    </div>
    {#if children}
        <div class={vertical ? "" : "flex shrink-0 items-center"}>
            {@render children()}
        </div>
    {/if}
</div>
