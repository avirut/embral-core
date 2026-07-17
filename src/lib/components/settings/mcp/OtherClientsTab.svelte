<script lang="ts">
    import SettingsGroup from "../SettingsGroup.svelte";
    import SettingRow from "../SettingRow.svelte";
    import CodeSnippet from "./CodeSnippet.svelte";
    import type { McpSetupInfo } from "./types";

    let { info }: { info: McpSetupInfo | null } = $props();
</script>

{#if info}
    <SettingsGroup label="Any MCP client">
        <SettingRow
            title="Server command"
            description="Point the client at this stdio binary."
            vertical
        >
            <CodeSnippet text={info.path} />
        </SettingRow>
        <SettingRow
            title="MCP configuration"
            description="For clients that take an mcpServers config block."
            vertical
        >
            <CodeSnippet text={info.config_json} block />
        </SettingRow>
    </SettingsGroup>
{/if}
