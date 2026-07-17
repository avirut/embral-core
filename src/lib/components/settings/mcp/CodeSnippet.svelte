<script lang="ts">
    import { Check, Copy } from "lucide-svelte";
    import { Button } from "$lib/components/ui/button";

    let { text, block = false }: { text: string; block?: boolean } = $props();
    let copied = $state(false);

    async function copy() {
        await navigator.clipboard.writeText(text);
        copied = true;
        setTimeout(() => (copied = false), 1500);
    }
</script>

<div class="flex w-full items-start gap-2">
    {#if block}
        <pre
            class="min-w-0 flex-1 overflow-x-auto rounded-md bg-muted px-3 py-2 font-mono text-xs">{text}</pre>
    {:else}
        <code
            class="min-w-0 flex-1 overflow-x-auto rounded-md bg-muted px-3 py-2 font-mono text-xs whitespace-nowrap"
            >{text}</code
        >
    {/if}
    <Button variant="outline" size="sm" onclick={copy}>
        {#if copied}<Check size={14} />{:else}<Copy size={14} />{/if}
    </Button>
</div>
