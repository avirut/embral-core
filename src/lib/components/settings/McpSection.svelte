<script lang="ts">
    import { onMount } from "svelte";
    import { invoke } from "@tauri-apps/api/core";
    import ClientCard from "./mcp/ClientCard.svelte";
    import CodeSnippet from "./mcp/CodeSnippet.svelte";
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
        Give your AI assistants read and search access to your
        meeting notes
    </p>

    {#if info && !info.exists}
        <div
            class="rounded-lg border border-border bg-muted/40 px-4 py-3 text-xs text-muted-foreground"
        >
            A part of embral this feature needs is missing (embral-mcp.exe).
            Reinstalling the app should fix this.
        </div>
    {/if}

    {#if semanticMissing}
        <div
            class="rounded-lg border border-border bg-muted/40 px-4 py-3 text-xs text-muted-foreground"
        >
            Assistants search by keywords only until the
            <button
                type="button"
                class="underline underline-offset-2 transition-colors hover:text-foreground"
                onclick={() => appState.openSettings("synthesis")}
                >Semantic search</button
            >
            model is downloaded — then they search by meaning too.
        </div>
    {/if}

    <div class="flex flex-col gap-3">
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
                        <code class="font-mono break-all"
                            >{info.claude_desktop_config_path}</code
                        >, then restart Claude Desktop:
                    </p>
                    <CodeSnippet text={info.config_json} block />
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
                    <p class="text-xs text-muted-foreground">
                        Run this once in your terminal:
                    </p>
                    <CodeSnippet text={info.claude_code_command} />
                {/if}
            {/snippet}
        </ClientCard>

        <ClientCard
            title="Codex"
            status={status?.codex}
            serverExists={status?.server_exists ?? false}
            action={act("codex")}
        >
            {#snippet fallback()}
                {#if info}
                    <p class="text-xs text-muted-foreground">
                        Run this once in your terminal:
                    </p>
                    <CodeSnippet text={info.codex_command} />
                {/if}
            {/snippet}
        </ClientCard>

        <ClientCard title="Other clients" subtitle="Connect any client by hand">
            {#snippet fallback()}
                {#if info}
                    <p class="text-xs text-muted-foreground">
                        Point the client at this server:
                    </p>
                    <CodeSnippet text={info.path} />
                    <p class="text-xs text-muted-foreground">
                        Or add this configuration:
                    </p>
                    <CodeSnippet text={info.config_json} block />
                {/if}
            {/snippet}
        </ClientCard>
    </div>
</div>
