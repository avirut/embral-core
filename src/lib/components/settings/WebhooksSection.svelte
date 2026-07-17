<script lang="ts">
    import { invoke } from "@tauri-apps/api/core";
    import type { AppConfig, WebhookMethod } from "$lib/types";
    import SettingsGroup from "./SettingsGroup.svelte";
    import SettingRow from "./SettingRow.svelte";
    import * as Select from "$lib/components/ui/select";
    import { Input } from "$lib/components/ui/input";
    import { Button } from "$lib/components/ui/button";

    let { draft }: { draft: AppConfig } = $props();

    let webhookTest = $state<"idle" | "sending" | "ok" | "error">("idle");
    let webhookTestError = $state("");

    async function sendTestWebhook() {
        webhookTest = "sending";
        webhookTestError = "";
        try {
            await invoke("test_webhook");
            webhookTest = "ok";
        } catch (e) {
            webhookTest = "error";
            webhookTestError = String(e);
        }
    }
</script>

<div class="space-y-6">
    <SettingsGroup label="Webhook">
        <SettingRow
            title="Send a webhook when a recording ends"
            description="Sends the meeting JSON to automation tools or your own scripts."
            vertical
        >
            <div class="flex w-full gap-2">
                <Select.Root
                    type="single"
                    value={draft.webhook_method}
                    onValueChange={(v) =>
                        (draft.webhook_method = (v ?? "post") as WebhookMethod)}
                >
                    <Select.Trigger class="w-24 uppercase"
                        >{draft.webhook_method}</Select.Trigger
                    >
                    <Select.Content>
                        <Select.Item value="post" label="POST" />
                        <Select.Item value="put" label="PUT" />
                    </Select.Content>
                </Select.Root>
                <Input
                    bind:value={draft.webhook_url}
                    placeholder="https://… (empty = off)"
                    class="flex-1"
                />
                <Button
                    variant="outline"
                    size="sm"
                    disabled={webhookTest === "sending" ||
                        !draft.webhook_url.trim()}
                    onclick={sendTestWebhook}
                >
                    {webhookTest === "sending" ? "Sending…" : "Send test"}
                </Button>
            </div>
            {#if webhookTest === "ok"}
                <p class="mt-1.5 text-xs text-primary">
                    Test delivered successfully.
                </p>
            {:else if webhookTest === "error"}
                <p class="mt-1.5 text-xs text-destructive">{webhookTestError}</p>
            {/if}
        </SettingRow>
    </SettingsGroup>

    <p class="px-1 text-xs text-muted-foreground">
        The payload: <span class="font-mono text-[11px]"
            >{"{ event, meeting: { id, title, date, duration_seconds }, notes_markdown, transcript_markdown }"}</span
        >
    </p>
</div>
