<script lang="ts">
    import { onMount } from "svelte";
    import { invoke } from "@tauri-apps/api/core";
    import * as Tabs from "$lib/components/ui/tabs";
    import ClaudeTab from "./mcp/ClaudeTab.svelte";
    import CodexTab from "./mcp/CodexTab.svelte";
    import OtherClientsTab from "./mcp/OtherClientsTab.svelte";
    import type {
        McpAction,
        McpClientId,
        McpClientsStatus,
        McpSetupInfo,
    } from "./mcp/types";
    import { appState } from "$lib/stores/app-state.svelte";
    import { modelsStore } from "$lib/stores/models.svelte";

    let info = $state<McpSetupInfo | null>(null);
    let status = $state<McpClientsStatus | null>(null);

    // Connected assistants search by meaning once the semantic model is
    // downloaded; until then this is the one nudge toward it.
    let semanticMissing = $derived(
        modelsStore.statuses.some((m) => m.kind === "embedding" && !m.present),
    );

    async function refresh() {
        try {
            status = await invoke<McpClientsStatus>("mcp_clients_status");
        } catch (e) {
            console.error("mcp_clients_status failed:", e);
        }
    }

    onMount(async () => {
        void modelsStore.refresh();
        try {
            [info] = await Promise.all([
                invoke<McpSetupInfo>("mcp_setup_info"),
                refresh(),
            ]);
        } catch (e) {
            console.error("mcp_setup_info failed:", e);
        }
    });

    // Every action ends by re-reading disk/CLI truth — "Registered" is
    // never assumed from a button click.
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
</script>

<div class="space-y-6">
    <p class="px-1 text-sm text-muted-foreground">
        embral ships a local MCP server so AI assistants can list, search, and
        read your meeting notes. It reads your library directly, so the app
        doesn't need to be running.
    </p>

    {#if info && !info.exists}
        <div
            class="rounded-lg border border-border bg-muted/40 px-4 py-3 text-xs text-muted-foreground"
        >
            The server binary isn't built yet. Run
            <code class="font-mono">cargo build --release -p embral-mcp</code>
            in the project folder first; installed builds include it
            automatically.
        </div>
    {/if}

    {#if semanticMissing}
        <div
            class="rounded-lg border border-border bg-muted/40 px-4 py-3 text-xs text-muted-foreground"
        >
            Assistants currently search by keywords only. Download the
            <button
                type="button"
                class="underline underline-offset-2 transition-colors hover:text-foreground"
                onclick={() => appState.openSettings("synthesis")}
                >Semantic search</button
            >
            model and they'll search by meaning too.
        </div>
    {/if}

    <Tabs.Root value="claude">
        <Tabs.List>
            <Tabs.Trigger value="claude">Claude</Tabs.Trigger>
            <Tabs.Trigger value="codex">ChatGPT & Codex</Tabs.Trigger>
            <Tabs.Trigger value="other">Other clients</Tabs.Trigger>
        </Tabs.List>
        <Tabs.Content value="claude" class="pt-4">
            <ClaudeTab {info} {status} {act} />
        </Tabs.Content>
        <Tabs.Content value="codex" class="pt-4">
            <CodexTab {info} {status} {act} />
        </Tabs.Content>
        <Tabs.Content value="other" class="pt-4">
            <OtherClientsTab {info} />
        </Tabs.Content>
    </Tabs.Root>
</div>
