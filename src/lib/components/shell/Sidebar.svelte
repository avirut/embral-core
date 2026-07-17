<script lang="ts">
    import { invoke } from "@tauri-apps/api/core";
    import {
        Mic,
        Menu,
        NotebookPen,
        BookUser,
        Speech,
        Settings,
    } from "lucide-svelte";
    import { appState, type AppView } from "$lib/stores/app-state.svelte";
    import { configStore } from "$lib/stores/config.svelte";
    import Tip from "$lib/components/Tip.svelte";
    import { CLOUD_ENABLED, loadHoursRing } from "$lib/cloud";
    import { cn } from "$lib/utils";

    // The rail expands/collapses via the hamburger at the bottom (persisted
    // as config.sidebar_expanded).
    let starting = $state(false);

    let expanded = $derived(configStore.config?.sidebar_expanded ?? false);

    // The recording view is its own destination (the record button); the
    // Meetings item covers the library and the import-processing view, and
    // stays reachable while a recording runs in the background.
    const meetingViews = ["idle", "processing"];

    const navItems: { view: AppView; label: string; icon: typeof Mic }[] = [
        { view: "idle", label: "Meetings", icon: NotebookPen },
        { view: "speakers", label: "Profiles", icon: BookUser },
        { view: "dictation", label: "Dictation", icon: Speech },
    ];

    function isActive(item: (typeof navItems)[number]): boolean {
        if (item.view === "idle") return meetingViews.includes(appState.view);
        return appState.view === item.view;
    }

    function navigate(item: (typeof navItems)[number]) {
        appState.setView(item.view);
    }

    async function onRecordClick() {
        if (appState.isRecording) {
            // Never stop from here — one stray click must not end a meeting.
            appState.setView("recording");
            return;
        }
        if (starting || !configStore.isConfigured) return;
        starting = true;
        try {
            await invoke("start_recording");
        } catch (e) {
            appState.setError(e instanceof Error ? e.message : String(e));
        } finally {
            starting = false;
        }
    }

    async function toggleExpanded() {
        const cfg = configStore.config;
        if (!cfg) return;
        await configStore.save({ ...cfg, sidebar_expanded: !expanded });
    }

    const rowClass =
        "flex h-10 w-full items-center gap-3 overflow-hidden rounded-lg px-[11px] text-sidebar-foreground/60 transition-colors hover:bg-sidebar-accent hover:text-sidebar-foreground";
    const activeClass = "bg-sidebar-accent text-sidebar-foreground";
    const labelClass = (show: boolean) =>
        cn(
            "min-w-0 truncate text-sm transition-[opacity,transform] duration-200",
            show ? "opacity-100" : "-translate-x-1 opacity-0",
        );
</script>

<nav
    class={cn(
        "flex shrink-0 flex-col justify-between overflow-hidden border-r border-sidebar-border bg-sidebar px-2 py-2 transition-[width] duration-200 ease-out",
        expanded ? "w-52" : "w-[54px]",
    )}
>
    <div class="flex flex-col gap-1">
        <!-- Record: the rail's headline action, and the recording view's own
             nav item while a recording runs (the rest of the app stays
             browsable; this button is the way back to the live view). -->
        <Tip
            side="right"
            text={appState.isRecording
                ? "Recording — open the live view"
                : configStore.isConfigured
                  ? "Start recording"
                  : "Configure transcription in Settings first"}
        >
            {#snippet children({ props })}
                <button
                    {...props}
                    class={cn(
                        rowClass,
                        appState.isRecording
                            ? "text-destructive hover:text-destructive"
                            : "bg-primary text-primary-foreground hover:bg-primary/90 hover:text-primary-foreground",
                        appState.isRecording &&
                            appState.view === "recording" &&
                            "bg-sidebar-accent",
                        (!configStore.isConfigured || starting) &&
                            !appState.isRecording &&
                            "cursor-default opacity-50 hover:bg-primary",
                    )}
                    onclick={onRecordClick}
                >
                    <Mic size={18} class="shrink-0" />
                    <span class={labelClass(expanded)}>
                        {appState.isRecording ? "Recording…" : "Record"}
                    </span>
                </button>
            {/snippet}
        </Tip>

        <div class="mx-1 my-1 border-t border-sidebar-border"></div>

        {#each navItems as item (item.view)}
            <Tip side="right" text={item.label}>
                {#snippet children({ props })}
                    <button
                        {...props}
                        class={cn(rowClass, isActive(item) && activeClass)}
                        aria-label={item.label}
                        onclick={() => navigate(item)}
                    >
                        <item.icon size={18} class="shrink-0" />
                        <span class={labelClass(expanded)}>{item.label}</span>
                    </button>
                {/snippet}
            </Tip>
        {/each}
    </div>

    <div class="flex flex-col gap-1">
        <!-- The cloud hours meter, in cloud builds only. It reaches the seam the
             way everything else does — through $lib/cloud, never by naming the
             directory (cloud-seam.md). It renders nothing when there is no
             account, so the rail is unchanged for offline users. -->
        {#if CLOUD_ENABLED}
            {#await loadHoursRing() then Ring}
                {#if Ring}
                    <Ring
                        {expanded}
                        rowClass={cn(rowClass, "hover:text-sidebar-foreground")}
                        labelClass={labelClass(expanded)}
                    />
                {/if}
            {/await}
        {/if}
        <Tip side="right" text="Settings">
            {#snippet children({ props })}
                <button
                    {...props}
                    class={cn(rowClass, appState.view === "settings" && activeClass)}
                    aria-label="Settings"
                    onclick={() => appState.setView("settings")}
                >
                    <Settings size={18} class="shrink-0" />
                    <span class={labelClass(expanded)}>Settings</span>
                </button>
            {/snippet}
        </Tip>
        <Tip side="right" text={expanded ? "Collapse sidebar" : "Expand sidebar"}>
            {#snippet children({ props })}
                <button
                    {...props}
                    class={rowClass}
                    aria-label={expanded ? "Collapse sidebar" : "Expand sidebar"}
                    aria-expanded={expanded}
                    onclick={toggleExpanded}
                >
                    <Menu size={18} class="shrink-0" />
                    <span class={labelClass(expanded)}>Collapse</span>
                </button>
            {/snippet}
        </Tip>
    </div>
</nav>
