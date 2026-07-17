<script lang="ts">
    import { onDestroy, onMount } from "svelte";
    import { listen, type UnlistenFn } from "@tauri-apps/api/event";
    import { Mic } from "lucide-svelte";
    import { configStore } from "$lib/stores/config.svelte";
    import { themeStore } from "$lib/stores/theme.svelte";
    import { formatCombo } from "$lib/utils/hotkey";

    // The dictation overlay window: never focused, always on top, shown by
    // the backend while a session runs. A minimal listening indicator — the
    // words land in the target app, not here.

    type Phase = "listening" | "finishing";
    let phase = $state<Phase>("listening");

    let unlisteners: UnlistenFn[] = [];

    onMount(async () => {
        await configStore.load();
        themeStore.apply(configStore.config?.theme ?? "system");
        unlisteners = await Promise.all([
            listen("dictation-started", () => {
                phase = "listening";
            }),
            listen("dictation-finishing", () => {
                phase = "finishing";
            }),
        ]);
    });

    onDestroy(() => {
        for (const fn of unlisteners) fn();
    });
</script>

<div
    class="flex h-screen w-screen flex-col overflow-hidden rounded-xl border border-border bg-background/95 text-foreground shadow-2xl"
>
    <div class="flex shrink-0 items-center gap-2 px-3 py-2">
        <span class="relative flex h-2.5 w-2.5">
            {#if phase === "listening"}
                <span
                    class="absolute inline-flex h-full w-full animate-ping rounded-full bg-destructive/60"
                ></span>
            {/if}
            <span class="relative inline-flex h-2.5 w-2.5 rounded-full bg-destructive"></span>
        </span>
        <Mic size={13} class="text-muted-foreground" />
        <span class="text-xs font-medium">
            {phase === "listening" ? "Listening…" : "Finishing up…"}
        </span>
        <span class="flex-1"></span>
        <span class="text-[10px] text-muted-foreground">
            {formatCombo(configStore.config?.dictation_hotkey ?? "")}
        </span>
    </div>
</div>
