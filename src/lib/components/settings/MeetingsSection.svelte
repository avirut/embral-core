<script lang="ts">
    import { onMount } from "svelte";
    import { invoke } from "@tauri-apps/api/core";
    import { Check, PenLine, RotateCcw } from "lucide-svelte";
    import * as Dialog from "$lib/components/ui/dialog";
    import { modelsStore } from "$lib/stores/models.svelte";
    import TranscriptionBlock from "./TranscriptionBlock.svelte";
    import HotkeyCapture from "./HotkeyCapture.svelte";
    import type {
        AppConfig,
        AutoStartPolicy,
        DiarizationSensitivity,
        OpenMeetingTab,
        SpeakerMatchMode,
    } from "$lib/types";
    import { BUILTIN_PROFILE_ID } from "$lib/types";
    import { CLOUD_ENABLED } from "$lib/cloud";
    import SettingsGroup from "./SettingsGroup.svelte";
    import SettingRow from "./SettingRow.svelte";
    import * as Select from "$lib/components/ui/select";
    import { Switch } from "$lib/components/ui/switch";
    import { Input } from "$lib/components/ui/input";
    import { Button } from "$lib/components/ui/button";
    import { Textarea } from "$lib/components/ui/textarea";
    import { cn } from "$lib/utils";

    let { draft }: { draft: AppConfig } = $props();

    // --- Auto-start -----------------------------------------------------

    const policyOptions: { value: AutoStartPolicy; label: string }[] = [
        { value: "always", label: "Always" },
        { value: "selective", label: "Selectively" },
        { value: "prompt", label: "After asking" },
        { value: "manual", label: "Never" },
    ];

    let policy = $derived(
        policyOptions.find((o) => o.value === draft.auto_start_policy) ??
            policyOptions[2],
    );

    // The fixed set of meeting apps (4×2 grid). Each checkbox owns one
    // process-match string; the detector matches case-insensitive substrings
    // ("teams" also catches ms-teams.exe).
    const knownApps: { label: string; match: string }[] = [
        { label: "Zoom", match: "zoom" },
        { label: "Teams", match: "teams" },
        { label: "Chrome", match: "chrome" },
        { label: "Edge", match: "msedge" },
        { label: "Firefox", match: "firefox" },
        { label: "Slack", match: "slack" },
        { label: "Discord", match: "discord" },
        { label: "Webex", match: "webex" },
    ];

    function appChecked(match: string): boolean {
        return draft.auto_detect_apps.includes(match);
    }

    function toggleApp(match: string) {
        draft.auto_detect_apps = appChecked(match)
            ? draft.auto_detect_apps.filter((a) => a !== match)
            : [...draft.auto_detect_apps, match];
    }


    // --- Speakers ------------------------------------------------------------

    const matchLabels: Record<SpeakerMatchMode, string> = {
        off: "Off",
        suggest: "Suggest",
        automatic: "Automatic",
    };

    const sensitivityLabels: Record<DiarizationSensitivity, string> = {
        low: "Fewer speakers",
        medium: "Balanced",
        high: "More speakers",
    };

    // --- Summaries prompt ----------------------------------------------------

    let defaultPrompt = $state("");
    let contract = $state("");
    let showContract = $state(false);
    let editorOpen = $state(false);

    onMount(async () => {
        modelsStore.refresh();
        try {
            const parts = await invoke<{ default: string; contract: string }>(
                "get_summary_prompt_parts",
            );
            defaultPrompt = parts.default;
            contract = parts.contract;
        } catch (e) {
            console.error("get_summary_prompt_parts failed:", e);
        }
    });

    // The summary engine (moved here from the Synthesis page — it is the
    // product question "who writes my summaries", not a model-management
    // knob). Engines are fixed per edition; the backend picks actual models.
    const CLOUD_PROFILE_ID = "cloud";
    let engineValue = $derived(draft.summaries_profile_id || BUILTIN_PROFILE_ID);
    let engineLabel = $derived(
        engineValue === CLOUD_PROFILE_ID ? "embral cloud" : "Built-in (on-device)",
    );

    // "" in config means "use the default" — the editor always shows the
    // effective text; the first edit materializes it as a custom prompt.
    let customized = $derived(draft.summary_prompt.trim().length > 0);
    let promptText = $derived(customized ? draft.summary_prompt : defaultPrompt);

    function onPromptInput(value: string) {
        // Typing the default back verbatim returns to "not customized".
        draft.summary_prompt = value.trim() === defaultPrompt.trim() ? "" : value;
    }

    const tabLabels: Record<OpenMeetingTab, string> = {
        summary: "Summary",
        notes: "Notes",
        transcript: "Transcript",
    };

    // "Open on Summary" is only an option while summaries exist at all; a
    // stored `summary` with the switch off rewrites to notes (same pattern
    // as the diarization↔model effect above).
    $effect(() => {
        if (!draft.summaries_enabled && draft.open_meeting_tab === "summary") {
            draft.open_meeting_tab = "notes";
        }
    });

    // --- Retention -----------------------------------------------------------

    const retentionOptions = [
        { value: "0", label: "Never" },
        { value: "7", label: "After 7 days" },
        { value: "30", label: "After 30 days" },
        { value: "90", label: "After 90 days" },
    ];

    let retentionLabel = $derived(
        retentionOptions.find(
            (o) => o.value === String(draft.audio_retention_days),
        )?.label ?? "Never",
    );

    const meetingRetentionOptions = [
        { value: "0", label: "Never" },
        { value: "90", label: "After 90 days" },
        { value: "365", label: "After 1 year" },
        { value: "730", label: "After 2 years" },
    ];

    let meetingRetentionLabel = $derived(
        meetingRetentionOptions.find(
            (o) => o.value === String(draft.meeting_retention_days),
        )?.label ?? "Never",
    );
</script>

<div class="space-y-6">
    <SettingsGroup label="Transcription">
        <TranscriptionBlock
            providerLabel="Transcribe meetings with"
            disabledNote="Recording and notes continue; no transcript is written."
            provider={draft.transcription_provider}
            onProviderChange={(v) => (draft.transcription_provider = v)}
            outOfHours={draft.cloud_out_of_hours}
            onOutOfHoursChange={(v) => (draft.cloud_out_of_hours = v)}
            language={draft.transcription_language}
            onLanguageChange={(v) => (draft.transcription_language = v)}
            accuracyModel={draft.local_asr_model}
            onAccuracyChange={(id) => (draft.local_asr_model = id)}
        />
    </SettingsGroup>

    <SettingsGroup label="Auto-start">
        <SettingRow title="When a call is detected, auto-start…">
            <Select.Root
                type="single"
                value={draft.auto_start_policy}
                onValueChange={(v) =>
                    (draft.auto_start_policy = (v ??
                        "prompt") as AutoStartPolicy)}
            >
                <Select.Trigger class="w-56">{policy.label}</Select.Trigger>
                <Select.Content>
                    {#each policyOptions as o (o.value)}
                        <Select.Item value={o.value} label={o.label} />
                    {/each}
                </Select.Content>
            </Select.Root>
        </SettingRow>

        {#if draft.auto_start_policy !== "manual"}
            {#if draft.auto_start_policy !== "always"}
                <SettingRow title="Meeting apps" vertical>
                    <div class="grid max-w-xl grid-cols-4 gap-2">
                        {#each knownApps as app (app.match)}
                            {@const checked = appChecked(app.match)}
                            <button
                                type="button"
                                class={cn(
                                    "flex items-center gap-2 rounded-md border px-2.5 py-1.5 text-left text-xs transition-colors",
                                    checked
                                        ? "border-primary/50 bg-primary/10"
                                        : "border-border hover:bg-accent/50",
                                )}
                                role="checkbox"
                                aria-checked={checked}
                                onclick={() => toggleApp(app.match)}
                            >
                                <span
                                    class={cn(
                                        "flex size-3.5 shrink-0 items-center justify-center rounded-sm border",
                                        checked
                                            ? "border-primary bg-primary text-primary-foreground"
                                            : "border-input",
                                    )}
                                >
                                    {#if checked}<Check size={10} strokeWidth={3} />{/if}
                                </span>
                                {app.label}
                            </button>
                        {/each}
                    </div>
                </SettingRow>
            {/if}

            <SettingRow
                title="Detection delay"
                description="Active microphone time before recording is triggered."
            >
                <div class="flex items-center gap-2">
                    <Input
                        type="number"
                        min="1"
                        max="60"
                        value={String(draft.detection_delay_secs)}
                        oninput={(e) =>
                            (draft.detection_delay_secs = Math.max(
                                1,
                                Number(e.currentTarget.value) || 1,
                            ))}
                        class="w-16 text-right"
                    />
                    <span class="text-xs text-muted-foreground">seconds</span>
                </div>
            </SettingRow>

            <SettingRow
                title="Stop when the call ends"
                description="Applies only to auto-started recordings."
            >
                <Switch bind:checked={draft.auto_stop_enabled} />
            </SettingRow>
        {/if}
    </SettingsGroup>

    <SettingsGroup label="Toggle recording">
        <SettingRow title="Hotkey">
            <HotkeyCapture
                value={draft.record_hotkey}
                ariaLabel="Record hotkey"
                onChange={(combo) => (draft.record_hotkey = combo)}
            />
        </SettingRow>
    </SettingsGroup>

    <SettingsGroup label="Speakers">
        <SettingRow title="Detect speakers">
            <Switch bind:checked={draft.diarization_enabled} />
        </SettingRow>

        {#if draft.diarization_enabled}
            <SettingRow title="Speaker separation">
                <Select.Root
                    type="single"
                    value={draft.diarization_sensitivity}
                    onValueChange={(v) =>
                        v &&
                        (draft.diarization_sensitivity =
                            v as DiarizationSensitivity)}
                >
                    <Select.Trigger class="w-56"
                        >{sensitivityLabels[
                            draft.diarization_sensitivity
                        ]}</Select.Trigger
                    >
                    <Select.Content>
                        <Select.Item value="low" label="Fewer speakers" />
                        <Select.Item value="medium" label="Balanced" />
                        <Select.Item value="high" label="More speakers" />
                    </Select.Content>
                </Select.Root>
            </SettingRow>

            <SettingRow title="Match voices from past meetings">
                <Select.Root
                    type="single"
                    value={draft.speaker_match_mode}
                    onValueChange={(v) =>
                        v && (draft.speaker_match_mode = v as SpeakerMatchMode)}
                >
                    <Select.Trigger class="w-56"
                        >{matchLabels[draft.speaker_match_mode]}</Select.Trigger
                    >
                    <Select.Content>
                        <Select.Item value="off" label="Off" />
                        <Select.Item value="suggest" label="Suggest" />
                        <Select.Item value="automatic" label="Automatic" />
                    </Select.Content>
                </Select.Root>
            </SettingRow>
        {/if}
    </SettingsGroup>

    <SettingsGroup label="Summaries">
        <SettingRow title="Summarize meetings">
            <Switch bind:checked={draft.summaries_enabled} />
        </SettingRow>

        {#if draft.summaries_enabled}
            <SettingRow
                title="Write summaries with"
                description={engineValue === CLOUD_PROFILE_ID
                    ? "Transcripts are summarized on embral's servers. Sign in on the Account page first."
                    : ""}
            >
                <Select.Root
                    type="single"
                    value={engineValue}
                    onValueChange={(v) => {
                        if (v) draft.summaries_profile_id = v;
                    }}
                >
                    <Select.Trigger class="w-56">{engineLabel}</Select.Trigger>
                    <Select.Content>
                        <Select.Item
                            value={BUILTIN_PROFILE_ID}
                            label="Built-in (on-device)"
                        />
                        {#if CLOUD_ENABLED}
                            <Select.Item value={CLOUD_PROFILE_ID} label="embral cloud" />
                        {/if}
                    </Select.Content>
                </Select.Root>
            </SettingRow>

            <SettingRow
                title="Summary prompt"
                description={customized ? "Customized." : ""}
            >
                <Button variant="outline" size="sm" onclick={() => (editorOpen = true)}>
                    <PenLine size={13} class="mr-1" /> Edit prompt…
                </Button>
            </SettingRow>
        {/if}

        <SettingRow title="Open meetings on">
            <Select.Root
                type="single"
                value={draft.open_meeting_tab}
                onValueChange={(v) =>
                    v && (draft.open_meeting_tab = v as OpenMeetingTab)}
            >
                <Select.Trigger class="w-56"
                    >{tabLabels[draft.open_meeting_tab]}</Select.Trigger
                >
                <Select.Content>
                    {#if draft.summaries_enabled}
                        <Select.Item value="summary" label="Summary" />
                    {/if}
                    <Select.Item value="notes" label="Notes" />
                    <Select.Item value="transcript" label="Transcript" />
                </Select.Content>
            </Select.Root>
        </SettingRow>
    </SettingsGroup>

    <SettingsGroup label="Audio recordings">
        <SettingRow title="Keep audio files">
            <Switch bind:checked={draft.retain_audio} />
        </SettingRow>

        <SettingRow title="Delete audio automatically">
            <Select.Root
                type="single"
                value={String(draft.audio_retention_days)}
                onValueChange={(v) =>
                    (draft.audio_retention_days = Number(v ?? "0"))}
            >
                <Select.Trigger class="w-56">{retentionLabel}</Select.Trigger>
                <Select.Content>
                    {#each retentionOptions as o (o.value)}
                        <Select.Item value={o.value} label={o.label} />
                    {/each}
                </Select.Content>
            </Select.Root>
        </SettingRow>

        <SettingRow title="Delete meetings automatically">
            <Select.Root
                type="single"
                value={String(draft.meeting_retention_days)}
                onValueChange={(v) =>
                    (draft.meeting_retention_days = Number(v ?? "0"))}
            >
                <Select.Trigger class="w-56"
                    >{meetingRetentionLabel}</Select.Trigger
                >
                <Select.Content>
                    {#each meetingRetentionOptions as o (o.value)}
                        <Select.Item value={o.value} label={o.label} />
                    {/each}
                </Select.Content>
            </Select.Root>
        </SettingRow>
    </SettingsGroup>
</div>

<Dialog.Root bind:open={editorOpen}>
    <Dialog.Content class="sm:max-w-3xl">
        <Dialog.Header>
            <Dialog.Title>Summary prompt</Dialog.Title>
            <Dialog.Description>
                The full prompt sent with every meeting. Edit anything; the
                required output format is appended automatically.
            </Dialog.Description>
        </Dialog.Header>
        <div class="space-y-2">
            <Textarea
                value={promptText}
                rows={16}
                class="max-h-[55vh] font-mono text-xs leading-relaxed"
                oninput={(e) => onPromptInput(e.currentTarget.value)}
            />
            <button
                class="text-xs text-muted-foreground underline-offset-2 hover:text-foreground hover:underline"
                onclick={() => (showContract = !showContract)}
            >
                {showContract ? "Hide" : "Show"} the enforced output format
            </button>
            {#if showContract}
                <pre
                    class="max-h-48 overflow-y-auto rounded-md border border-border bg-muted/40 p-3 font-mono text-[11px] leading-relaxed whitespace-pre-wrap text-muted-foreground">{contract}</pre>
            {/if}
        </div>
        <Dialog.Footer class="items-center gap-2 sm:justify-between">
            <div>
                {#if customized}
                    <Button
                        variant="outline"
                        size="sm"
                        onclick={() => (draft.summary_prompt = "")}
                    >
                        <RotateCcw size={13} class="mr-1" /> Reset to default
                    </Button>
                {/if}
            </div>
            <Button size="sm" onclick={() => (editorOpen = false)}>Done</Button>
        </Dialog.Footer>
    </Dialog.Content>
</Dialog.Root>
