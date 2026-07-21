<script lang="ts">
    // Dictation's day-one decisions: the hotkey that makes it exist, the
    // cleanup mode, and where the text lands. The provider/language tree
    // keeps its defaults; Settings has the rest.
    import * as Select from "$lib/components/ui/select";
    import { Switch } from "$lib/components/ui/switch";
    import SettingRow from "$lib/components/settings/SettingRow.svelte";
    import SettingsGroup from "$lib/components/settings/SettingsGroup.svelte";
    import HotkeyCapture from "$lib/components/settings/HotkeyCapture.svelte";
    import { cloudAuth } from "$lib/stores/cloudAuth.svelte";
    import { CLOUD_ENABLED } from "$lib/cloud";
    import type { DictationCleanup } from "$lib/types";
    import type { OnboardingDraft } from "../types";

    let { draft }: { draft: OnboardingDraft } = $props();

    const cleanupLabels: Record<string, string> = {
        cloud: "embral cloud",
        on_device: "local model",
        off: "no cleanup",
    };
</script>

<h1 class="font-display text-2xl tracking-tight">Dictation</h1>
<p class="mt-3 text-sm text-muted-foreground">
    Speech to text in any app, with realtime feedback
</p>

<div class="mt-6 space-y-4">
    <SettingsGroup>
        <SettingRow title="Dictation hotkey">
            <HotkeyCapture
                value={draft.dictation_hotkey}
                ariaLabel="Dictation hotkey"
                onChange={(combo) => (draft.dictation_hotkey = combo)}
            />
        </SettingRow>

        <SettingRow
            title="Clean up with AI"
            description="Remove filler words and fix phrasing"
        >
            <Select.Root
                type="single"
                value={draft.dictation_cleanup}
                onValueChange={(v) => {
                    if (!v) return;
                    if (v === "cloud" && !cloudAuth.requireSignedIn()) return;
                    draft.dictation_cleanup = v as DictationCleanup;
                }}
            >
                <Select.Trigger class="w-44"
                    >{cleanupLabels[draft.dictation_cleanup]}</Select.Trigger
                >
                <Select.Content>
                    {#if CLOUD_ENABLED}
                        <Select.Item value="cloud" label="embral cloud" />
                    {/if}
                    <Select.Item value="on_device" label="local model" />
                    <Select.Item value="off" label="no cleanup" />
                </Select.Content>
            </Select.Root>
        </SettingRow>

        <SettingRow title="Copy to clipboard">
            <Switch bind:checked={draft.dictation_copy_clipboard} />
        </SettingRow>

        <SettingRow title="Paste into the active app">
            <Switch bind:checked={draft.dictation_auto_paste} />
        </SettingRow>
    </SettingsGroup>
</div>
