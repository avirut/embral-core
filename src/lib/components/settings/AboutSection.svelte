<script lang="ts">
    import { onMount } from "svelte";
    import { invoke } from "@tauri-apps/api/core";
    import { getVersion } from "@tauri-apps/api/app";
    import { FolderOpen, Loader2, RotateCcw } from "lucide-svelte";
    import { openUrl as openUrlExternal } from "@tauri-apps/plugin-opener";
    import * as Dialog from "$lib/components/ui/dialog";
    import SettingsGroup from "./SettingsGroup.svelte";
    import SettingRow from "./SettingRow.svelte";
    import { Button } from "$lib/components/ui/button";
    import { Switch } from "$lib/components/ui/switch";
    import EmbralIcon from "$lib/components/EmbralIcon.svelte";
    import { openNotesFolder } from "$lib/utils/openNotesFolder";
    import { configStore } from "$lib/stores/config.svelte";
    import { appState } from "$lib/stores/app-state.svelte";
    import { meetingsStore } from "$lib/stores/meetings.svelte";
    import { dictationStore } from "$lib/stores/dictation.svelte";
    import { modelsStore } from "$lib/stores/models.svelte";
    import { updaterStore } from "$lib/stores/updater.svelte";

    let version = $state("");
    let resetOpen = $state(false);
    let resetting = $state(false);
    let resetError = $state("");

    // Settings is the old one-button reset, so it starts on.
    const scopeItems = [
        { key: "settings", label: "Settings" },
        { key: "meetings", label: "Meetings" },
        { key: "profiles", label: "Profiles" },
        { key: "dictations", label: "Dictations" },
        { key: "models", label: "Models" },
    ] as const;
    type ScopeKey = (typeof scopeItems)[number]["key"];
    let scopes = $state<Record<ScopeKey, boolean>>({
        settings: true,
        meetings: false,
        profiles: false,
        dictations: false,
        models: false,
    });
    let anyScope = $derived(scopeItems.some((s) => scopes[s.key]));

    async function resetApp() {
        resetting = true;
        resetError = "";
        try {
            await invoke("reset_app_data", { scopes: { ...scopes } });
            if (scopes.meetings) await meetingsStore.load();
            if (scopes.dictations) await dictationStore.refresh();
            if (scopes.models) await modelsStore.refresh();
            resetOpen = false;
            if (scopes.settings) {
                // Reloading picks up onboarding_completed=false, so the
                // onboarding gate takes over the window.
                await configStore.load();
                appState.setView("idle");
            }
        } catch (e) {
            resetError = e instanceof Error ? e.message : String(e);
        } finally {
            resetting = false;
        }
    }

    onMount(async () => {
        try {
            version = await getVersion();
        } catch {
            version = "dev";
        }
    });

    async function openLogs() {
        try {
            await invoke("open_logs_folder");
        } catch (e) {
            console.error("open_logs_folder failed:", e);
        }
    }

    async function openUrl(url: string) {
        try {
            await openUrlExternal(url);
        } catch (e) {
            console.error("open url failed:", e);
        }
    }

    const credits: { name: string; what: string; license: string; url: string }[] = [
        {
            name: "sherpa-onnx (k2-fsa)",
            what: "speech runtime",
            license: "Apache-2.0",
            url: "https://github.com/k2-fsa/sherpa-onnx",
        },
        {
            name: "NVIDIA Parakeet & TitaNet",
            what: "speech recognition & speaker embeddings",
            license: "CC-BY-4.0",
            url: "https://huggingface.co/nvidia",
        },
        {
            name: "icefall Zipformer",
            what: "speech recognition",
            license: "Apache-2.0",
            url: "https://github.com/k2-fsa/icefall",
        },
        {
            name: "pyannote segmentation",
            what: "speaker diarization",
            license: "MIT",
            url: "https://github.com/pyannote/pyannote-audio",
        },
        {
            name: "NVIDIA TitaNet",
            what: "voice recognition",
            license: "CC-BY-4.0",
            url: "https://catalog.ngc.nvidia.com/orgs/nvidia/teams/nemo/models/titanet_small",
        },
        {
            name: "Silero VAD",
            what: "voice activity detection",
            license: "MIT",
            url: "https://github.com/snakers4/silero-vad",
        },
        {
            name: "Qwen3 (Alibaba)",
            what: "summaries & dictation cleanup",
            license: "Apache-2.0",
            url: "https://github.com/QwenLM/Qwen3",
        },
        {
            name: "llama.cpp",
            what: "on-device LLM runtime",
            license: "MIT",
            url: "https://github.com/ggml-org/llama.cpp",
        },
    ];
</script>

<div class="space-y-6">
    <div class="flex items-center gap-4 rounded-lg border border-border p-5">
        <EmbralIcon size={36} />
        <div class="min-w-0">
            <p class="text-base font-semibold tracking-tight">embral</p>
            <p class="mt-0.5 text-xs text-muted-foreground">
                Meetings, transcripts, and dictation that stay on your
                computer.
            </p>
            <p class="mt-1 font-mono text-[11px] text-muted-foreground/80">
                version {version}
            </p>
        </div>
    </div>

    <SettingsGroup label="Updates">
        <SettingRow
            title={updaterStore.available
                ? `Version ${updaterStore.available.version} is ready`
                : "Up to date"}
            description={updaterStore.blocked
                ? `${updaterStore.blocked} — finish it, then update.`
                : ""}
        >
            {#if updaterStore.available}
                <Button
                    size="sm"
                    onclick={() => updaterStore.install()}
                    disabled={updaterStore.installing}
                >
                    {#if updaterStore.installing}
                        <Loader2 size={13} class="animate-spin" />
                        Installing…
                    {:else}
                        <RotateCcw size={13} />
                        Restart and update
                    {/if}
                </Button>
            {:else}
                <Button
                    variant="outline"
                    size="sm"
                    onclick={() => updaterStore.checkNow()}
                    disabled={updaterStore.checking}
                >
                    {#if updaterStore.checking}
                        <Loader2 size={13} class="animate-spin" />
                    {/if}
                    Check for updates
                </Button>
            {/if}
        </SettingRow>
        {#if updaterStore.error}
            <p class="px-4 pb-3 text-xs text-destructive">{updaterStore.error}</p>
        {/if}
    </SettingsGroup>

    <SettingsGroup label="Diagnostics">
        <SettingRow
            title="Logs"
            description="Attach the latest file when reporting a problem."
        >
            <Button variant="outline" size="sm" onclick={openLogs}>
                <FolderOpen size={14} />
                Open logs folder
            </Button>
        </SettingRow>
        <SettingRow title="Notes folder">
            <Button variant="outline" size="sm" onclick={openNotesFolder}>
                <FolderOpen size={14} />
                Open notes folder
            </Button>
        </SettingRow>
        <SettingRow title="Reset app">
            <Button
                variant="outline"
                size="sm"
                class="text-destructive hover:text-destructive"
                disabled={resetting}
                onclick={() => (resetOpen = true)}
            >
                <RotateCcw size={14} />
                Reset…
            </Button>
        </SettingRow>
    </SettingsGroup>

    <SettingsGroup label="On the shoulders of">
        <div class="px-4 py-3">
            <p class="text-xs text-muted-foreground">
                Everything embral does on your machine runs on openly shared
                models and runtimes:
            </p>
            <div class="mt-2.5 space-y-1.5">
                {#each credits as c (c.name)}
                    <div class="flex items-baseline justify-between gap-3 text-xs">
                        <button
                            class="min-w-0 truncate text-left text-foreground underline-offset-2 hover:underline"
                            onclick={() => openUrl(c.url)}
                        >
                            {c.name}
                        </button>
                        <span class="shrink-0 text-muted-foreground"
                            >{c.what} · {c.license}</span
                        >
                    </div>
                {/each}
            </div>
        </div>
    </SettingsGroup>
</div>

<Dialog.Root bind:open={resetOpen}>
    <Dialog.Content class="sm:max-w-md">
        <Dialog.Header>
            <Dialog.Title>Reset embral</Dialog.Title>
            <Dialog.Description>
                Pick what to erase. None of it can be brought back.
            </Dialog.Description>
        </Dialog.Header>
        <div>
            {#each scopeItems as item (item.key)}
                <div class="flex items-center justify-between py-2">
                    <span class="text-sm">{item.label}</span>
                    <Switch bind:checked={scopes[item.key]} />
                </div>
            {/each}
        </div>
        {#if resetError}
            <p class="text-xs text-destructive">{resetError}</p>
        {/if}
        <Dialog.Footer>
            <Button variant="ghost" size="sm" onclick={() => (resetOpen = false)}>
                Cancel
            </Button>
            <Button
                variant="destructive"
                size="sm"
                disabled={!anyScope || resetting}
                onclick={resetApp}
            >
                {resetting ? "Resetting…" : "Reset"}
            </Button>
        </Dialog.Footer>
    </Dialog.Content>
</Dialog.Root>
