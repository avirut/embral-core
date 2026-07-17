<script lang="ts">
    import { onMount } from "svelte";
    import { invoke } from "@tauri-apps/api/core";
    import { open } from "@tauri-apps/plugin-dialog";
    import { RefreshCw } from "lucide-svelte";
    import type { AppConfig, AudioDevices, Theme } from "$lib/types";
    import SettingsGroup from "./SettingsGroup.svelte";
    import SettingRow from "./SettingRow.svelte";
    import * as Select from "$lib/components/ui/select";
    import { Switch } from "$lib/components/ui/switch";
    import { Input } from "$lib/components/ui/input";
    import { Button } from "$lib/components/ui/button";

    let { draft }: { draft: AppConfig } = $props();

    const themeLabels: Record<Theme, string> = {
        system: "System",
        light: "Light",
        dark: "Dark",
    };

    // --- Audio devices (moved from the former Audio page) ---
    // Sentinel for "system default" — an empty string is stored in config.
    const DEFAULT = "__default__";
    let devices = $state<AudioDevices>({ inputs: [], outputs: [] });
    let loading = $state(false);

    async function refresh() {
        loading = true;
        try {
            devices = await invoke<AudioDevices>("list_audio_devices");
        } catch (e) {
            console.error("list_audio_devices failed:", e);
        } finally {
            loading = false;
        }
    }

    onMount(refresh);

    function deviceLabel(configured: string): string {
        return configured === "" ? "System default" : configured;
    }

    async function browseStorageDir() {
        const dir = await open({ directory: true });
        if (typeof dir === "string") {
            draft.storage_dir = dir;
        }
    }
</script>

<div class="space-y-6">
    <SettingsGroup label="Appearance">
        <SettingRow title="Color scheme">
            <Select.Root
                type="single"
                value={draft.theme}
                onValueChange={(v) => (draft.theme = (v ?? "system") as Theme)}
            >
                <Select.Trigger class="w-56"
                    >{themeLabels[draft.theme]}</Select.Trigger
                >
                <Select.Content>
                    <Select.Item value="system" label="System" />
                    <Select.Item value="light" label="Light" />
                    <Select.Item value="dark" label="Dark" />
                </Select.Content>
            </Select.Root>
        </SettingRow>
    </SettingsGroup>

    <SettingsGroup label="Storage">
        <SettingRow
            title="Storage folder"
            description="Where your meetings, transcripts, and notes live."
            vertical
        >
            <div class="flex w-full gap-2">
                <Input bind:value={draft.storage_dir} class="flex-1" />
                <Button variant="outline" size="sm" onclick={browseStorageDir}
                    >Browse…</Button
                >
            </div>
        </SettingRow>
    </SettingsGroup>

    <SettingsGroup label="Audio">
        <SettingRow title="Microphone">
            <Select.Root
                type="single"
                value={draft.mic_device === "" ? DEFAULT : draft.mic_device}
                onValueChange={(v) =>
                    (draft.mic_device = !v || v === DEFAULT ? "" : v)}
            >
                <Select.Trigger class="w-56"
                    >{deviceLabel(draft.mic_device)}</Select.Trigger
                >
                <Select.Content>
                    <Select.Item value={DEFAULT} label="System default" />
                    {#each devices.inputs as name (name)}
                        <Select.Item value={name} label={name} />
                    {/each}
                </Select.Content>
            </Select.Root>
        </SettingRow>

        <SettingRow
            title="System audio"
            description="Lets calls on headphones still record everyone."
        >
            <Select.Root
                type="single"
                value={draft.output_device === ""
                    ? DEFAULT
                    : draft.output_device}
                onValueChange={(v) =>
                    (draft.output_device = !v || v === DEFAULT ? "" : v)}
            >
                <Select.Trigger class="w-56"
                    >{deviceLabel(draft.output_device)}</Select.Trigger
                >
                <Select.Content>
                    <Select.Item value={DEFAULT} label="System default" />
                    {#each devices.outputs as name (name)}
                        <Select.Item value={name} label={name} />
                    {/each}
                </Select.Content>
            </Select.Root>
        </SettingRow>

        <SettingRow title="Refresh devices">
            <Button
                variant="outline"
                size="sm"
                disabled={loading}
                onclick={refresh}
            >
                <RefreshCw size={14} class={loading ? "animate-spin" : ""} />
                Refresh
            </Button>
        </SettingRow>
    </SettingsGroup>

    <SettingsGroup label="Notifications">
        <SettingRow title="Summary ready">
            <Switch bind:checked={draft.notify_summary_ready} />
        </SettingRow>
        <SettingRow
            title="Recording started"
            description="Only when the window is hidden."
        >
            <Switch bind:checked={draft.notify_recording_started} />
        </SettingRow>
        <SettingRow
            title="Call detected"
            description="Only when embral is set to ask before recording."
        >
            <Switch bind:checked={draft.notify_call_detected} />
        </SettingRow>
        <SettingRow
            title="Update ready"
            description="A new version is available to install."
        >
            <Switch bind:checked={draft.notify_update_available} />
        </SettingRow>
    </SettingsGroup>
</div>
