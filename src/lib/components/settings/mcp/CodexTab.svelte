<script lang="ts">
    import ClientCard from "./ClientCard.svelte";
    import CodeSnippet from "./CodeSnippet.svelte";
    import type { McpAction, McpClientId, McpClientsStatus, McpSetupInfo } from "./types";

    let {
        info,
        status,
        act,
    }: {
        info: McpSetupInfo | null;
        status: McpClientsStatus | null;
        act: (client: McpClientId) => McpAction;
    } = $props();
</script>

<div class="space-y-3">
    <ClientCard
        title="Codex"
        status={status?.codex}
        serverExists={status?.server_exists ?? false}
        action={act("codex")}
    >
        {#snippet fallback()}
            {#if info}
                <p class="text-xs text-muted-foreground">Run once in your terminal:</p>
                <CodeSnippet text={info.codex_command} />
                <p class="text-xs text-muted-foreground">
                    Or add this to <code class="font-mono">~\.codex\config.toml</code>:
                </p>
                <CodeSnippet text={info.codex_toml} block />
            {/if}
        {/snippet}
    </ClientCard>

    <p class="px-1 text-xs text-muted-foreground">
        ChatGPT itself can't connect: it only accepts remote servers reached
        over HTTPS, and your library lives on this machine. Codex is the
        OpenAI-side path today.
    </p>
</div>
