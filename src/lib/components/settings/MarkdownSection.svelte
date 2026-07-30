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
    import { copy } from "$lib/copy";

    let { draft }: { draft: AppConfig } = $props();

    const t = $derived(copy.settings.markdown);

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

    // Token codes are parsed by Rust (data); the meaning beside each comes
    // from the catalog by key.
    const tokens: {
        token: string;
        key: keyof typeof copy.settings.markdown.filename.tokens;
    }[] = [
        { token: "{date}", key: "date" },
        { token: "{time}", key: "time" },
        { token: "{year}", key: "year" },
        { token: "{month}", key: "month" },
        { token: "{day}", key: "day" },
        { token: "{hour}", key: "hour" },
        { token: "{minute}", key: "minute" },
        { token: "{title}", key: "title" },
    ];
</script>

<div class="space-y-6">
    <SettingsGroup label={t.autoExport._group}>
        <SettingRow
            title={t.autoExport.enabled.label}
            description={t.autoExport.enabled.sub}
        >
            <Switch bind:checked={draft.obsidian_export_enabled} />
        </SettingRow>
        {#if draft.obsidian_export_enabled}
            <SettingRow title={t.autoExport.folder.label} vertical>
                <div class="flex w-full gap-2">
                    <Input
                        bind:value={draft.obsidian_vault_dir}
                        placeholder={t.autoExport.folder.placeholder}
                        class="flex-1"
                    />
                    <Button variant="outline" size="sm" onclick={browseVaultDir}
                        >{t.autoExport.browse}</Button
                    >
                </div>
            </SettingRow>
        {/if}
    </SettingsGroup>

    <SettingsGroup label={t.include._group}>
        <SettingRow title={t.include.summary.label}>
            <Switch bind:checked={draft.export_include_summary} />
        </SettingRow>
        <SettingRow title={t.include.notes.label}>
            <Switch bind:checked={draft.export_include_notes} />
        </SettingRow>
        <SettingRow title={t.include.transcript.label}>
            <Switch bind:checked={draft.export_include_transcript} />
        </SettingRow>
    </SettingsGroup>

    <SettingsGroup label={t.filename._group}>
        <SettingRow title={t.filename.template.label} vertical>
            <Input
                bind:value={draft.export_filename_template}
                class="max-w-md font-mono text-xs"
            />
            {#if preview}
                <p class="mt-1.5 text-xs text-muted-foreground">
                    {t.filename.preview}
                    <span class="font-mono">{preview}</span>
                </p>
            {/if}
            <div class="mt-2 grid max-w-md grid-cols-2 gap-x-6 gap-y-0.5 text-xs text-muted-foreground">
                {#each tokens as { token, key } (token)}
                    <div class="flex justify-between gap-3">
                        <span class="font-mono">{token}</span>
                        <span>{t.filename.tokens[key]}</span>
                    </div>
                {/each}
            </div>
        </SettingRow>

        <SettingRow title={t.filename.metadata.label}>
            <Select.Root
                type="single"
                value={draft.export_metadata_format}
                onValueChange={(v) =>
                    (draft.export_metadata_format = (v ??
                        "frontmatter") as ExportMetadataFormat)}
            >
                <Select.Trigger class="w-56"
                    >{t.filename.metadata.options[
                        draft.export_metadata_format
                    ]}</Select.Trigger
                >
                <Select.Content>
                    <Select.Item
                        value="frontmatter"
                        label={t.filename.metadata.options.frontmatter}
                    />
                    <Select.Item
                        value="inline"
                        label={t.filename.metadata.options.inline}
                    />
                </Select.Content>
            </Select.Root>
        </SettingRow>
    </SettingsGroup>
</div>
