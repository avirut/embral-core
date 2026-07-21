<script lang="ts">
    // The feature grid: six things embral does, few words each. Greyscale —
    // emphasis is contrast, not color ([shell.md](../../../../../docs/shell.md)).
    import { onMount } from "svelte";
    import type { Component } from "svelte";
    import {
        Bot,
        FileText,
        Lock,
        NotebookPen,
        Speech,
        Users,
    } from "lucide-svelte";
    import { loadTelemetryOptIn } from "$lib/cloud";

    // The telemetry opt-in checkbox is a cloud-edition component
    // ([telemetry.md]); the open-core build renders nothing here.
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    let TelemetryOptIn = $state<Component<any> | null>(null);
    onMount(async () => {
        TelemetryOptIn = await loadTelemetryOptIn();
    });

    const features = [
        {
            icon: NotebookPen,
            title: "Meetings",
            body: "Record, transcribe, summarize, & query",
        },
        {
            icon: Speech,
            title: "Dictation",
            body: "Speak to type anywhere, with cleanup",
        },
        {
            icon: Users,
            title: "Profiles",
            body: "Track speakers across your meetings",
        },
        {
            icon: FileText,
            title: "Markdown",
            body: "Export notes to Obsidian & other tools",
        },
        {
            icon: Bot,
            title: "AI assistants",
            body: "Let Claude and Codex search your notes",
        },
        {
            icon: Lock,
            title: "Private",
            body: "Your notes & dictations stay on your computer",
        },
    ];
</script>

<h1 class="font-display text-2xl tracking-tight">Welcome to embral</h1>
<p class="mt-3 text-sm text-muted-foreground">
    Private notes & dictation integrated with the tools you already use:
</p>

<div class="mt-6 grid grid-cols-2 gap-3 sm:grid-cols-3">
    {#each features as f (f.title)}
        <div class="rounded-lg border border-border bg-card p-3">
            <f.icon size={16} class="text-foreground/80" />
            <p class="mt-2 text-sm font-medium">{f.title}</p>
            <p class="mt-0.5 text-xs leading-relaxed text-muted-foreground">
                {f.body}
            </p>
        </div>
    {/each}
</div>

{#if TelemetryOptIn}
    <TelemetryOptIn />
{/if}
