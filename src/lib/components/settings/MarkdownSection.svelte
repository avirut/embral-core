<script lang="ts">
    import { invoke } from "@tauri-apps/api/core";
    import { open } from "@tauri-apps/plugin-dialog";
    import type { AppConfig, ExportMetadataFormat } from "$lib/types";
    import SettingsGroup from "./SettingsGroup.svelte";
    import SettingRow from "./SettingRow.svelte";
    import * as Select from "$lib/components/ui/select";
    import { Switch } from "$lib/components/ui/switch";
    import { Input } from "$lib/components/ui/input";
    import { Button } from "$lib/components/ui/button";

    let { draft }: { draft: AppConfig } = $props();

    let preview = $state("");

    // Live preview rendered by the same Rust template code real exports use.
    $effect(() => {
        const template = draft.export_filename_template;
        invoke<string>("preview_export_filename", { template })
            .then((p) => (preview = p))
            .catch(() => (preview = ""));
    });

    async function browseVaultDir() {
        const dir = await open({ directory: true });
        if (typeof dir === "string") {
            draft.obsidian_vault_dir = dir;
        }
    }

    const formatLabels: Record<ExportMetadataFormat, string> = {
        frontmatter: "YAML frontmatter",
        inline: "Inline",
    };

    const tokens = [
        ["{date}", "YYYY-MM-DD"],
        ["{time}", "HH-MM"],
        ["{year}", "YYYY"],
        ["{month}", "MM"],
        ["{day}", "DD"],
        ["{hour}", "HH"],
        ["{minute}", "MM"],
        ["{title}", "meeting title (slugified)"],
    ] as const;
</script>

<div class="space-y-6">
    <SettingsGroup label="Auto-export">
        <SettingRow
            title="Export notes when a recording ends"
            description="Saves a markdown copy into a folder of your choice, like an Obsidian vault."
        >
            <Switch bind:checked={draft.obsidian_export_enabled} />
        </SettingRow>
        {#if draft.obsidian_export_enabled}
            <SettingRow title="Export folder" vertical>
                <div class="flex w-full gap-2">
                    <Input
                        bind:value={draft.obsidian_vault_dir}
                        placeholder="Path to your vault or notes folder"
                        class="flex-1"
                    />
                    <Button variant="outline" size="sm" onclick={browseVaultDir}
                        >Browse…</Button
                    >
                </div>
            </SettingRow>
        {/if}
    </SettingsGroup>

    <SettingsGroup label="Filename">
        <SettingRow
            title="Filename template"
            description="Applies to exported copies only."
            vertical
        >
            <Input
                bind:value={draft.export_filename_template}
                class="max-w-md font-mono text-xs"
            />
            {#if preview}
                <p class="mt-1.5 text-xs text-muted-foreground">
                    Preview:
                    <span class="font-mono">{preview}</span>
                </p>
            {/if}
            <div class="mt-2 grid max-w-md grid-cols-2 gap-x-6 gap-y-0.5 text-xs text-muted-foreground">
                {#each tokens as [token, meaning] (token)}
                    <div class="flex justify-between gap-3">
                        <span class="font-mono">{token}</span>
                        <span>{meaning}</span>
                    </div>
                {/each}
            </div>
        </SettingRow>

        <SettingRow
            title="Metadata format"
            description="Frontmatter suits Obsidian; Inline writes a readable block under the heading."
        >
            <Select.Root
                type="single"
                value={draft.export_metadata_format}
                onValueChange={(v) =>
                    (draft.export_metadata_format = (v ??
                        "frontmatter") as ExportMetadataFormat)}
            >
                <Select.Trigger class="w-56"
                    >{formatLabels[draft.export_metadata_format]}</Select.Trigger
                >
                <Select.Content>
                    <Select.Item value="frontmatter" label="YAML frontmatter" />
                    <Select.Item value="inline" label="Inline" />
                </Select.Content>
            </Select.Root>
        </SettingRow>
    </SettingsGroup>
</div>
