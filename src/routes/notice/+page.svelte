<script lang="ts">
    // The notice window's page ([shell.md] §Notices) — embral's own
    // notification chrome on Windows. A separate WebView from main: it
    // loads config and theme itself and installs its own listeners, like
    // the dictation overlay.
    //
    // One shape for every notice: the logo, one line of text, the answers.
    // Every interaction acknowledges instantly: actions fire without being
    // awaited, the card fades on its own clock, and Record swaps to a
    // "starting" line until the recording-started event takes it down.
    import { onDestroy, onMount } from "svelte";
    import { invoke } from "@tauri-apps/api/core";
    import { listen, type UnlistenFn } from "@tauri-apps/api/event";
    import { X } from "lucide-svelte";
    import { configStore } from "$lib/stores/config.svelte";
    import { themeStore } from "$lib/stores/theme.svelte";
    import EmbralIcon from "$lib/components/EmbralIcon.svelte";
    import { copy } from "$lib/copy";
    import { cn } from "$lib/utils";

    const t = $derived(copy.notifications.os.callDetected);

    type NoticeAction = { id: string; label: string };
    type NoticePayload = {
        kind: string;
        title: string;
        actions: NoticeAction[];
        sticky: boolean;
        target?: { kind: string; id?: string } | null;
    };

    let notice = $state<NoticePayload | null>(null);
    let fading = $state(false);
    let starting = $state(false);
    let unlisteners: UnlistenFn[] = [];

    // Action ids → the commands they run; the answers are the same ones
    // the in-app banners invoke, so telemetry and state ride along.
    const ACTION_COMMANDS: Record<string, string> = {
        accept: "accept_detected_meeting",
        dismiss: "dismiss_detected_meeting",
        keep: "silence_keep_recording",
        stop: "request_stop_recording",
    };

    // Transient notices dismiss themselves; hovering holds them open.
    const DISMISS_MS = 8000;
    const FADE_MS = 180;
    let dismissTimer: ReturnType<typeof setTimeout> | null = null;
    let fadeTimer: ReturnType<typeof setTimeout> | null = null;

    function armDismiss() {
        cancelDismiss();
        if (!notice || notice.sticky || fading) return;
        dismissTimer = setTimeout(fadeThenHide, DISMISS_MS);
    }

    function cancelDismiss() {
        if (dismissTimer) {
            clearTimeout(dismissTimer);
            dismissTimer = null;
        }
    }

    /// The only way off screen: a quick fade, then the backend hides the
    /// window (an instant hide() reads as the card snapping out).
    function fadeThenHide() {
        if (fading) return;
        fading = true;
        cancelDismiss();
        fadeTimer = setTimeout(() => void invoke("hide_notice").catch(() => {}), FADE_MS);
    }

    function runAction(id: string) {
        const command = ACTION_COMMANDS[id];
        if (command) {
            // Never awaited — the click's feedback must not wait on model
            // spin-up or the stop handshake.
            void invoke(command).catch(() => {});
        }
        if (id === "accept") {
            // Held on screen as "starting" until recording-started lands.
            starting = true;
        } else {
            fadeThenHide();
        }
    }

    /// A sticky notice answered elsewhere (the in-app banner, the call
    /// ending) comes down here too.
    function hideIf(kind: string) {
        if (notice?.kind !== kind) return;
        fadeThenHide();
    }

    /// The ✕. On the call notice it IS the Dismiss answer (a separate
    /// Dismiss button next to a close would say the same thing twice);
    /// everywhere else it just puts the card away.
    function close() {
        if (notice?.kind === "call_detected") {
            void invoke("dismiss_detected_meeting").catch(() => {});
        }
        fadeThenHide();
    }

    function openTarget() {
        if (!notice || starting) return;
        const target = notice.target ?? { kind: "app" };
        void invoke("open_from_notice", { target }).catch(() => {});
    }

    function takePayload(payload: NoticePayload) {
        notice = payload;
        fading = false;
        starting = false;
        if (fadeTimer) {
            clearTimeout(fadeTimer);
            fadeTimer = null;
        }
        armDismiss();
    }

    onMount(async () => {
        await configStore.load();
        themeStore.apply(configStore.config?.theme ?? "system");
        unlisteners = await Promise.all([
            listen<NoticePayload>("notice-payload", (e) => takePayload(e.payload)),
            listen("silence-cleared", () => hideIf("silence")),
            listen("recording-stopped", () => hideIf("silence")),
            listen("meeting-ended", () => hideIf("call_detected")),
            listen("meeting-dismissed", () => hideIf("call_detected")),
            listen("recording-started", () => hideIf("call_detected")),
        ]);
        // The first show races this page's load — the emit that triggered
        // window creation is long gone by the time the listener above
        // exists. The backend keeps the payload; ask for it.
        if (!notice) {
            const pending = await invoke<NoticePayload | null>("current_notice").catch(
                () => null,
            );
            if (pending && !notice) takePayload(pending);
        }
    });

    onDestroy(() => {
        for (const u of unlisteners) u();
        cancelDismiss();
        if (fadeTimer) clearTimeout(fadeTimer);
    });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -- hover only holds
     the auto-dismiss; the card's controls carry their own semantics. -->
<div
    class={cn(
        "flex h-screen w-screen items-center gap-2.5 overflow-hidden rounded-xl border border-border bg-background/95 px-4 text-foreground shadow-2xl transition-opacity duration-200",
        fading && "opacity-0",
    )}
    onpointerenter={cancelDismiss}
    onpointerleave={armDismiss}
>
    {#if notice}
        <EmbralIcon size={18} />
        <button class="min-w-0 flex-1 text-left" onclick={openTarget}>
            <p class="truncate text-sm font-medium">
                {starting ? t.starting : notice.title}
            </p>
        </button>
        {#if !starting}
            {#each notice.actions as action, i (action.id)}
                <button
                    class={i === 0
                        ? "shrink-0 rounded-md bg-primary px-3 py-1 text-xs font-medium text-primary-foreground transition-colors hover:bg-primary/90"
                        : "shrink-0 rounded-md px-2 py-1 text-xs text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"}
                    onclick={() => runAction(action.id)}
                >
                    {action.label}
                </button>
            {/each}
            <button
                class="shrink-0 rounded p-1 text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
                aria-label={copy.common.close}
                onclick={close}
            >
                <X size={14} />
            </button>
        {/if}
    {/if}
</div>
