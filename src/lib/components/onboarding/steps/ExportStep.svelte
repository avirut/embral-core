<script lang="ts">
    // Markdown export: the switch and the folder. Template editing stays in Settings
    import { invoke } from "@tauri-apps/api/core";
    import { open } from "@tauri-apps/plugin-dialog";
    import { Switch } from "$lib/components/ui/switch";
    import { Button } from "$lib/components/ui/button";
    import SettingRow from "$lib/components/settings/SettingRow.svelte";
    import SettingsGroup from "$lib/components/settings/SettingsGroup.svelte";
    import type { OnboardingDraft } from "../types";

    let { draft }: { draft: OnboardingDraft } = $props();

    async function pickFolder() {
        const dir = await open({ directory: true });
        if (typeof dir === "string") {
            draft.obsidian_vault_dir = dir;
            // Picking a folder is the intent — don't make them find the
            // switch too.
            draft.obsidian_export_enabled = true;
        }
    }
</script>

<h1 class="font-display text-2xl tracking-tight">Connect to your knowledge base</h1>
<p class="mt-3 text-sm text-muted-foreground">
    Export meeting notes as markdown to a local folder, like an Obsidian vault
</p>

<div class="mt-6 space-y-4">
    <SettingsGroup>
        <SettingRow title="Export notes when a recording ends">
            <Switch bind:checked={draft.obsidian_export_enabled} />
        </SettingRow>

        <SettingRow
            title="Folder"
            description={draft.obsidian_vault_dir || "No folder chosen yet."}
        >
            <Button variant="outline" size="sm" onclick={pickFolder}>
                Browse…
            </Button>
        </SettingRow>

        {#if draft.obsidian_export_enabled}
            <SettingRow title="Include the AI summary">
                <Switch bind:checked={draft.export_include_summary} />
            </SettingRow>
            <SettingRow title="Include your notes">
                <Switch bind:checked={draft.export_include_notes} />
            </SettingRow>
            <SettingRow title="Include the transcript">
                <Switch bind:checked={draft.export_include_transcript} />
            </SettingRow>
        {/if}

        {#if draft.obsidian_export_enabled}
            <div class="px-4 py-3">
                <p class="text-xs text-muted-foreground">
                    Adjust filename format in settings
                </p>
            </div>
        {/if}
    </SettingsGroup>
</div>
