<script lang="ts">
    // The Accessibility-permission hint under the auto-paste switch
    // (settings and onboarding). Renders only where the OS gates synthetic
    // keystrokes (macOS) and only while auto-paste is on but the
    // permission is missing; the request shows the system's one-time
    // prompt, after which the grant lives in the OS privacy pane
    // ([dictation.md](../../../../docs/dictation.md)).
    import { onMount } from "svelte";
    import { invoke } from "@tauri-apps/api/core";
    import { openUrl } from "@tauri-apps/plugin-opener";
    import { Keyboard } from "lucide-svelte";
    import { Button } from "$lib/components/ui/button";
    import { copy } from "$lib/copy";
    import type { PermissionState } from "$lib/types";

    let { enabled }: { enabled: boolean } = $props();

    const t = $derived(copy.common.axAccess);

    let permission = $state<PermissionState | null>(null);
    let asked = $state(false);

    onMount(async () => {
        try {
            permission = await invoke<PermissionState>("accessibility_permission");
        } catch (e) {
            console.error("accessibility_permission failed:", e);
        }
    });

    async function request() {
        asked = true;
        try {
            permission = await invoke<PermissionState>(
                "request_accessibility_permission",
            );
        } catch (e) {
            console.error("request_accessibility_permission failed:", e);
        }
    }

    const AX_PRIVACY_PANE =
        "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";
</script>

{#if enabled && permission === "denied"}
    <div class="flex items-center justify-between gap-3 px-4 py-3">
        <div class="flex items-center gap-3">
            <Keyboard class="size-4 shrink-0 text-muted-foreground" />
            <p class="text-xs text-muted-foreground">{t.needed}</p>
        </div>
        {#if asked}
            <Button
                variant="outline"
                size="sm"
                onclick={() => void openUrl(AX_PRIVACY_PANE)}
            >
                {t.openSettings}
            </Button>
        {:else}
            <Button variant="outline" size="sm" onclick={request}>
                {t.allow}
            </Button>
        {/if}
    </div>
{/if}
