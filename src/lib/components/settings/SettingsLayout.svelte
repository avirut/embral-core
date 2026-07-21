<script lang="ts">
    import { onMount } from "svelte";
    import {
        SettingsIcon,
        Mic,
        Disc,
        AudioLines,
        Network,
        Info,
        FileText,
        Speech,
        Brain,
        CircleUser,
    } from "lucide-svelte";
    import { CLOUD_ENABLED, loadAccountSection } from "$lib/cloud";
    import type { AppConfig } from "$lib/types";
    import { appState } from "$lib/stores/app-state.svelte";
    import { settingsForm } from "$lib/stores/settings-form.svelte";
    import { cloudAuth } from "$lib/stores/cloudAuth.svelte";
    import { cn } from "$lib/utils";
    import GeneralSection from "./GeneralSection.svelte";
    import MeetingsSection from "./MeetingsSection.svelte";
    import DictationSection from "./DictationSection.svelte";
    import AboutSection from "./AboutSection.svelte";
    import MarkdownSection from "./MarkdownSection.svelte";
    import McpSection from "./McpSection.svelte";
    import TranscriptionSection from "./TranscriptionSection.svelte";
    import SynthesisSection from "./SynthesisSection.svelte";
    import CloudSignInDialog from "./CloudSignInDialog.svelte";

    type SectionId =
        | "account"
        | "general"
        | "meetings"
        | "dictation"
        | "about"
        | "markdown"
        | "mcp"
        | "transcription"
        | "synthesis";

    type Entry = { id: SectionId; label: string; icon: typeof Mic };
    const groups: { label: string; items: Entry[] }[] = [
        {
            label: "Application",
            items: [
                { id: "general", label: "General", icon: SettingsIcon },
                { id: "meetings", label: "Meetings", icon: Disc },
                { id: "dictation", label: "Dictation", icon: Speech },
                // Account exists only in the cloud edition; it sits with About
                // — the two pages about you rather than about the app's work.
                ...(CLOUD_ENABLED
                    ? [{ id: "account", label: "Account", icon: CircleUser } as Entry]
                    : []),
                { id: "about", label: "About", icon: Info },
            ],
        },
        {
            label: "Models",
            items: [
                { id: "transcription", label: "Transcription", icon: AudioLines },
                { id: "synthesis", label: "Synthesis", icon: Brain },
            ],
        },
        {
            label: "Integrations",
            items: [
                { id: "markdown", label: "Markdown", icon: FileText },
                { id: "mcp", label: "MCP", icon: Network },
            ],
        },
    ];
    const sections: Entry[] = groups.flatMap((g) => g.items);

    let active = $state<SectionId>("general");
    let initialized = false;

    onMount(() => {
        settingsForm.reset();
        // Know the cloud sign-in state before the user reaches a provider
        // selector, and keep it current when they sign in or out on the
        // Account page (which dispatches this event).
        if (!CLOUD_ENABLED) return;
        void cloudAuth.refresh();
        const onCloudChanged = () => void cloudAuth.refresh();
        window.addEventListener("embral:cloud-changed", onCloudChanged);
        return () =>
            window.removeEventListener("embral:cloud-changed", onCloudChanged);
    });

    // Palette deep links: land on the requested page (also fires when a
    // deep link arrives while settings is already open).
    $effect(() => {
        const target = appState.settingsTarget;
        if (target && sections.some((s) => s.id === target)) {
            active = target as SectionId;
            appState.clearSettingsTarget();
        }
    });

    // Snapshot the whole draft (spread reads every field) so any change in any
    // section schedules the shared debounced save. The first run after reset
    // is the initialization pass, not an edit.
    $effect(() => {
        const draft = settingsForm.draft;
        if (!draft) return;
        const snapshot: AppConfig = { ...draft };
        if (!initialized) {
            initialized = true;
            return;
        }
        settingsForm.scheduleSave(snapshot);
    });

</script>

{#if settingsForm.draft}
    <div class="flex min-h-0 flex-1">
        <nav class="w-52 shrink-0 space-y-4 overflow-y-auto border-r border-border p-3">
            {#each groups as group (group.label)}
                <div>
                    <p
                        class="px-2.5 pb-1 text-[10px] font-semibold tracking-widest text-muted-foreground/70 uppercase"
                    >
                        {group.label}
                    </p>
                    <div class="space-y-0.5">
                        {#each group.items as s (s.id)}
                            <button
                                class={cn(
                                    "flex w-full items-center gap-2.5 rounded-md px-2.5 py-1.5 text-left text-sm text-muted-foreground transition-colors hover:bg-accent hover:text-foreground",
                                    active === s.id &&
                                        "bg-accent font-medium text-foreground",
                                )}
                                onclick={() => (active = s.id)}
                            >
                                <s.icon size={15} />
                                {s.label}
                            </button>
                        {/each}
                    </div>
                </div>
            {/each}
        </nav>

        <div class="min-w-0 flex-1 overflow-y-auto">
            <div class="mx-auto max-w-3xl px-6 py-6">
                <h2 class="mb-5 text-lg font-semibold tracking-tight">
                    {sections.find((s) => s.id === active)?.label}
                </h2>
                {#if active === "account"}
                    {#await loadAccountSection() then Account}
                        {#if Account}<Account />{/if}
                    {/await}
                {:else if active === "general"}
                    <GeneralSection draft={settingsForm.draft} />
                {:else if active === "meetings"}
                    <MeetingsSection draft={settingsForm.draft} />
                {:else if active === "dictation"}
                    <DictationSection draft={settingsForm.draft} />
                {:else if active === "about"}
                    <AboutSection />
                {:else if active === "markdown"}
                    <MarkdownSection draft={settingsForm.draft} />
                {:else if active === "mcp"}
                    <McpSection />
                {:else if active === "transcription"}
                    <TranscriptionSection draft={settingsForm.draft} />
                {:else if active === "synthesis"}
                    <SynthesisSection draft={settingsForm.draft} />
                {/if}
            </div>
        </div>
    </div>

    {#if CLOUD_ENABLED}
        <CloudSignInDialog />
    {/if}
{:else}
    <div class="flex flex-1 items-center justify-center">
        <p class="text-sm text-muted-foreground">Loading…</p>
    </div>
{/if}
