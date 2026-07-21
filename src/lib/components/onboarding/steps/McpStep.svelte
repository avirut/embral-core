<script lang="ts">
    // Connect the AI clients this machine already has. Detection-first: only
    // installed clients get a card (the settings page keeps the full list
    // with manual fallbacks, [integrations.md](../../../../../docs/integrations.md)).
    import { onMount } from "svelte";
    import { invoke } from "@tauri-apps/api/core";
    import ClientCard from "$lib/components/settings/mcp/ClientCard.svelte";
    import type {
        McpAction,
        McpClientId,
        McpClientsStatus,
    } from "$lib/components/settings/mcp/types";

    let status = $state<McpClientsStatus | null>(null);

    async function refresh() {
        try {
            status = await invoke<McpClientsStatus>("mcp_clients_status");
        } catch (e) {
            console.error("mcp_clients_status failed:", e);
        }
    }

    onMount(refresh);

    // Same contract as Settings → MCP: every action re-reads disk/CLI truth.
    function act(client: McpClientId): McpAction {
        return async (kind) => {
            try {
                return await invoke<string>(
                    kind === "register" ? "mcp_register" : "mcp_unregister",
                    { client },
                );
            } finally {
                await refresh();
            }
        };
    }

    let installed = $derived.by(() => {
        if (!status) return [];
        const entries: { id: McpClientId; title: string }[] = [
            { id: "claude_desktop", title: "Claude Desktop" },
            { id: "claude_code", title: "Claude Code" },
            { id: "codex", title: "Codex" },
        ];
        return entries.filter((e) => status![e.id].installed);
    });
</script>

<h1 class="font-display text-2xl tracking-tight">Connect your AI assistants</h1>
<p class="mt-3 text-sm text-muted-foreground">
    Search your meeting notes; other MCP clients can be added in settings
</p>

<div class="mt-6 space-y-3">
    {#if status === null}
        <p class="text-sm text-muted-foreground">Looking for installed clients…</p>
    {:else if installed.length === 0}
        <p class="text-sm text-muted-foreground">
            No supported AI clients found on this machine. Set this up any time
            in Settings → MCP.
        </p>
    {:else}
        {#each installed as client (client.id)}
            <ClientCard
                title={client.title}
                status={status[client.id]}
                serverExists={status.server_exists}
                action={act(client.id)}
            />
        {/each}
    {/if}
</div>
