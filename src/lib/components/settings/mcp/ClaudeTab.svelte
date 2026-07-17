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
        title="Claude Desktop"
        status={status?.claude_desktop}
        serverExists={status?.server_exists ?? false}
        action={act("claude_desktop")}
    >
        {#snippet fallback()}
            {#if info}
                <p class="text-xs text-muted-foreground">
                    Add this to
                    <code class="font-mono">{info.claude_desktop_config_path}</code>
                    and restart Claude Desktop:
                </p>
                <CodeSnippet text={info.config_json} block />
                <p class="text-xs text-muted-foreground">
                    Or install it as an extension:
                    <code class="font-mono">npm run build:mcpb</code>
                    produces <code class="font-mono">dist\embral.mcpb</code> —
                    drag it into Claude Desktop → Settings → Extensions.
                </p>
            {/if}
        {/snippet}
    </ClientCard>

    <ClientCard
        title="Claude Code"
        status={status?.claude_code}
        serverExists={status?.server_exists ?? false}
        action={act("claude_code")}
    >
        {#snippet fallback()}
            {#if info}
                <p class="text-xs text-muted-foreground">Run once in your terminal:</p>
                <CodeSnippet text={info.claude_code_command} />
            {/if}
        {/snippet}
    </ClientCard>
</div>
