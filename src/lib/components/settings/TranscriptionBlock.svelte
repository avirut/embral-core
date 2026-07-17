<script lang="ts">
    // The transcription decision tree as settings rows: provider,
    // out-of-hours behavior, language, and the on-device accuracy tier.
    // Meetings and Dictation each have their own copy of these fields in
    // config, so the block binds through values + onChange props rather
    // than a draft — one component, two trees. (Dictation uses it today;
    // the Meetings section migrates onto it in the P3 restyle.)
    import type {
        CloudOutOfHours,
        TranscriptionLanguage,
        TranscriptionProvider,
    } from "$lib/types";
    import SettingRow from "./SettingRow.svelte";
    import SpeechModelPicker from "./SpeechModelPicker.svelte";
    import * as Select from "$lib/components/ui/select";
    import { CLOUD_ENABLED } from "$lib/cloud";

    let {
        providerLabel,
        cloudNote = "",
        disabledNote = "",
        provider,
        onProviderChange,
        outOfHours,
        onOutOfHoursChange,
        language,
        onLanguageChange,
        accuracyModel,
        onAccuracyChange,
    }: {
        /** "Dictate with" / "Transcribe meetings with". */
        providerLabel: string;
        /** Shown under the provider row while cloud is selected. */
        cloudNote?: string;
        /** What "Disable transcription" means for this surface. */
        disabledNote?: string;
        provider: TranscriptionProvider;
        onProviderChange: (v: TranscriptionProvider) => void;
        outOfHours: CloudOutOfHours | undefined;
        onOutOfHoursChange: (v: CloudOutOfHours) => void;
        language: TranscriptionLanguage;
        onLanguageChange: (v: TranscriptionLanguage) => void;
        /** The effective English model id the accuracy picker shows. */
        accuracyModel: string;
        onAccuracyChange: (id: string) => void;
    } = $props();

    // The device transcribes when it is the primary, or where an
    // out-of-hours cloud session lands. Only cloud-with-"disable" never
    // needs the accuracy tier (or the model behind it).
    let deviceTranscribes = $derived(
        !CLOUD_ENABLED || provider !== "cloud" || outOfHours !== "disabled",
    );
</script>

{#if CLOUD_ENABLED}
    <SettingRow
        title={providerLabel}
        description={provider === "cloud" ? cloudNote : ""}
    >
        <Select.Root
            type="single"
            value={provider}
            onValueChange={(v) => {
                if (v === "local" || v === "cloud") onProviderChange(v);
            }}
        >
            <Select.Trigger class="w-56"
                >{provider === "cloud" ? "embral cloud" : "This device"}</Select.Trigger
            >
            <Select.Content>
                <Select.Item value="local" label="This device" />
                <Select.Item value="cloud" label="embral cloud" />
            </Select.Content>
        </Select.Root>
    </SettingRow>

    {#if provider === "cloud"}
        <SettingRow
            title="When cloud hours run out"
            description={outOfHours === "disabled" ? disabledNote : ""}
        >
            <Select.Root
                type="single"
                value={outOfHours ?? "local"}
                onValueChange={(v) => {
                    if (v === "local" || v === "disabled") onOutOfHoursChange(v);
                }}
            >
                <Select.Trigger class="w-56"
                    >{outOfHours === "disabled"
                        ? "Disable transcription"
                        : "Switch to this device"}</Select.Trigger
                >
                <Select.Content>
                    <Select.Item value="local" label="Switch to this device" />
                    <Select.Item value="disabled" label="Disable transcription" />
                </Select.Content>
            </Select.Root>
        </SettingRow>
    {/if}
{/if}

<SettingRow title="Language">
    <Select.Root
        type="single"
        value={language}
        onValueChange={(v) => {
            if (v === "english" || v === "multilingual") onLanguageChange(v);
        }}
    >
        <Select.Trigger class="w-56"
            >{language === "multilingual" ? "All languages" : "English"}</Select.Trigger
        >
        <Select.Content>
            <Select.Item value="english" label="English" />
            <Select.Item value="multilingual" label="All languages" />
        </Select.Content>
    </Select.Root>
</SettingRow>

<!-- The accuracy tier is an English, on-device concept: multilingual has
     exactly one model (downloaded from the models page), and cloud-with-
     "disable" never touches the device. -->
{#if deviceTranscribes && language === "english"}
    <SpeechModelPicker
        value={accuracyModel}
        onChange={onAccuracyChange}
        {language}
    />
{/if}
