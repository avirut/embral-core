<script lang="ts">
    import { onMount } from "svelte";
    import { Info } from "lucide-svelte";
    import type { AppConfig, DictationCleanup } from "$lib/types";
    import TranscriptionBlock from "./TranscriptionBlock.svelte";
    import HotkeyCapture from "./HotkeyCapture.svelte";
    import SettingsGroup from "./SettingsGroup.svelte";
    import SettingRow from "./SettingRow.svelte";
    import * as Dialog from "$lib/components/ui/dialog";
    import * as Select from "$lib/components/ui/select";
    import { Input } from "$lib/components/ui/input";
    import { Switch } from "$lib/components/ui/switch";
    import { modelsStore } from "$lib/stores/models.svelte";
    import { CLOUD_ENABLED } from "$lib/cloud";
    import { cloudAuth } from "$lib/stores/cloudAuth.svelte";

    let { draft }: { draft: AppConfig } = $props();

    onMount(() => {
        modelsStore.refresh();
    });

    // "" historically meant "same as meetings"; the UI now always shows a
    // concrete model (the backend still falls back while the value is "").
    let effectiveDictationModel = $derived(
        draft.dictation_asr_model || draft.local_asr_model,
    );

    const cleanupLabels: Record<DictationCleanup, string> = {
        cloud: "embral cloud",
        on_device: "local model",
        off: "no cleanup",
    };
    let cleanupInfoOpen = $state(false);
</script>

<div class="space-y-6">
    <SettingsGroup label="Start dictating">
        <SettingRow title="Hotkey">
            <HotkeyCapture
                value={draft.dictation_hotkey}
                ariaLabel="Dictation hotkey"
                onChange={(combo) => (draft.dictation_hotkey = combo)}
            />
        </SettingRow>
    </SettingsGroup>

    <!-- Dictation's own transcription tree — independent of the Meetings
         one, because cloud meetings with on-device dictation is legitimate. -->
    <SettingsGroup label="Transcription">
        <TranscriptionBlock
            providerLabel="Dictate with"
            provider={draft.dictation_provider}
            onProviderChange={(v) => (draft.dictation_provider = v)}
            outOfHours={draft.dictation_out_of_hours}
            onOutOfHoursChange={(v) => (draft.dictation_out_of_hours = v)}
            language={draft.dictation_language}
            onLanguageChange={(v) => (draft.dictation_language = v)}
            accuracyModel={effectiveDictationModel}
            onAccuracyChange={(id) => (draft.dictation_asr_model = id)}
        />
    </SettingsGroup>

    <SettingsGroup label="Output">
        <SettingRow title="Copy to clipboard">
            <Switch bind:checked={draft.dictation_copy_clipboard} />
        </SettingRow>
        <SettingRow title="Auto-paste on completion">
            <Switch bind:checked={draft.dictation_auto_paste} />
        </SettingRow>
        <SettingRow title="Clean up with AI">
            {#snippet titleExtra()}
                <button
                    class="text-muted-foreground/60 transition-colors hover:text-foreground"
                    aria-label="What cleanup does"
                    onclick={() => (cleanupInfoOpen = true)}
                >
                    <Info size={13} />
                </button>
            {/snippet}
            <Select.Root
                type="single"
                value={draft.dictation_cleanup}
                onValueChange={(v) => {
                    if (!v) return;
                    // Cloud cleanup needs an account; refuse and prompt when
                    // signed out, leaving cleanup on its current value.
                    if (v === "cloud" && !cloudAuth.requireSignedIn()) return;
                    draft.dictation_cleanup = v as DictationCleanup;
                }}
            >
                <Select.Trigger class="w-56"
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
    </SettingsGroup>

    <SettingsGroup label="History">
        <SettingRow
            title="Auto-delete history"
            description={draft.dictation_auto_delete
                ? "Use 0 to ignore a criterion."
                : ""}
        >
            <Switch bind:checked={draft.dictation_auto_delete} />
        </SettingRow>
        {#if draft.dictation_auto_delete}
            <SettingRow title="Delete after">
                <div class="flex items-center gap-2">
                    <Input
                        type="number"
                        min="0"
                        bind:value={draft.dictation_retention_days}
                        class="w-16 text-right"
                    />
                    <span class="text-xs text-muted-foreground">days</span>
                </div>
            </SettingRow>
            <SettingRow title="Keep only the last">
                <div class="flex items-center gap-2">
                    <Input
                        type="number"
                        min="0"
                        bind:value={draft.dictation_retention_count}
                        class="w-16 text-right"
                    />
                    <span class="text-xs text-muted-foreground">dictations</span>
                </div>
            </SettingRow>
        {/if}
    </SettingsGroup>
</div>

<Dialog.Root bind:open={cleanupInfoOpen}>
    <Dialog.Content class="sm:max-w-lg">
        <Dialog.Header>
            <Dialog.Title>What cleanup does</Dialog.Title>
            <Dialog.Description>
                Your dictation goes to the model as-is; what comes back is what
                gets pasted. A failure of any kind delivers the raw
                transcription instead — cleanup never loses your words.
            </Dialog.Description>
        </Dialog.Header>
        <div class="space-y-4 text-sm">
            <div>
                <p class="font-medium">Punctuation and fillers</p>
                <p class="mt-1 text-xs text-muted-foreground">
                    "um so i think we should uh move the meeting to thursday"
                </p>
                <p class="mt-0.5 text-xs">
                    → I think we should move the meeting to Thursday.
                </p>
            </div>
            <div>
                <p class="font-medium">Spoken formatting</p>
                <p class="mt-1 text-xs text-muted-foreground">
                    "first item new line second item new paragraph and that's it"
                </p>
                <p class="mt-0.5 text-xs">
                    → First item<br />Second item<br /><br />And that's it.
                </p>
            </div>
            <div>
                <p class="font-medium">Instruction mode</p>
                <p class="mt-1 text-xs text-muted-foreground">
                    Open with an instruction and it follows it: "make a bulleted
                    list milk eggs flour"
                </p>
                <p class="mt-0.5 text-xs">→ • Milk&ensp;• Eggs&ensp;• Flour</p>
            </div>
        </div>
    </Dialog.Content>
</Dialog.Root>
