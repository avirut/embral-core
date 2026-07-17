<script lang="ts">
    import type { Snippet } from "svelte";
    import { ChevronDown, ChevronRight } from "lucide-svelte";
    import { Button } from "$lib/components/ui/button";
    import type { ClientStatus, McpAction } from "./types";

    let {
        title,
        status = null,
        serverExists,
        action,
        fallbackLabel = "Manual setup",
        fallback,
    }: {
        title: string;
        status?: ClientStatus | null;
        serverExists: boolean;
        action: McpAction;
        fallbackLabel?: string;
        fallback?: Snippet;
    } = $props();

    let busy = $state(false);
    let message = $state<string | null>(null);
    let error = $state<string | null>(null);
    let fallbackOpen = $state(false);

    // A client we can't automate leads with the manual path, not a dead
    // button; so does a failed action.
    $effect(() => {
        if (status && !status.installed) fallbackOpen = true;
    });

    async function run(kind: "register" | "unregister") {
        busy = true;
        message = null;
        error = null;
        try {
            message = await action(kind);
        } catch (e) {
            error = String(e);
            fallbackOpen = true;
        } finally {
            busy = false;
        }
    }
</script>

<div class="rounded-lg border border-border p-4">
    <div class="flex items-start justify-between gap-3">
        <div class="min-w-0">
            <div class="flex items-center gap-2">
                <span class="text-sm font-medium">{title}</span>
                {#if status?.registered}
                    <span
                        class="text-[10px] font-semibold tracking-widest text-primary uppercase"
                        >Registered</span
                    >
                {/if}
            </div>
            <p
                class="mt-0.5 truncate text-xs text-muted-foreground"
                title={status?.detail}
            >
                {status?.detail ?? "Checking…"}
            </p>
        </div>
        {#if status?.installed}
            {#if status.registered}
                <Button
                    variant="outline"
                    size="sm"
                    disabled={busy}
                    onclick={() => run("unregister")}
                >
                    {busy ? "Working…" : "Remove"}
                </Button>
            {:else}
                <Button
                    size="sm"
                    disabled={busy || !serverExists}
                    onclick={() => run("register")}
                >
                    {busy ? "Working…" : "Register"}
                </Button>
            {/if}
        {/if}
    </div>

    {#if message}
        <p class="mt-2 text-xs text-primary">{message}</p>
    {/if}
    {#if error}
        <p class="mt-2 text-xs text-destructive">{error}</p>
    {/if}

    {#if fallback}
        <button
            type="button"
            class="mt-3 flex items-center gap-1 text-xs text-muted-foreground transition-colors hover:text-foreground"
            onclick={() => (fallbackOpen = !fallbackOpen)}
        >
            {#if fallbackOpen}<ChevronDown size={12} />{:else}<ChevronRight
                    size={12}
                />{/if}
            {fallbackLabel}
        </button>
        {#if fallbackOpen}
            <div class="mt-2 space-y-2">
                {@render fallback()}
            </div>
        {/if}
    {/if}
</div>
