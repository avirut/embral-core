<script lang="ts">
    // The microphone-permission card (shared: onboarding's meetings step
    // and Settings → General beside the device pickers). Renders nothing
    // where the OS doesn't gate the mic (Windows) or once access is
    // granted; a denial deep-links to the OS privacy pane
    // ([shell.md](../../../../docs/shell.md)).
    import { onMount } from "svelte";
    import { invoke } from "@tauri-apps/api/core";
    import { openUrl } from "@tauri-apps/plugin-opener";
    import { Mic } from "lucide-svelte";
    import { Button } from "$lib/components/ui/button";
    import { copy } from "$lib/copy";
    import type { PermissionState } from "$lib/types";

    const t = $derived(copy.common.micAccess);

    let permission = $state<PermissionState | null>(null);

    onMount(async () => {
        try {
            permission = await invoke<PermissionState>("mic_permission");
        } catch (e) {
            console.error("mic_permission failed:", e);
        }
    });

    async function request() {
        try {
            permission = await invoke<PermissionState>("request_mic_permission");
        } catch (e) {
            console.error("request_mic_permission failed:", e);
        }
    }

    const MIC_PRIVACY_PANE =
        "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone";
</script>

{#if permission === "not_determined" || permission === "denied"}
    <div class="flex items-center justify-between gap-3 rounded-lg border border-border bg-muted/40 px-4 py-3">
        <div class="flex items-center gap-3">
            <Mic class="size-4 shrink-0 text-muted-foreground" />
            <p class="text-xs text-muted-foreground">
                {permission === "denied" ? t.denied : t.ask}
            </p>
        </div>
        {#if permission === "denied"}
            <Button
                variant="outline"
                size="sm"
                onclick={() => void openUrl(MIC_PRIVACY_PANE)}
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
