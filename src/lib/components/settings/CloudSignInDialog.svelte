<script lang="ts">
    // Shown when the user tries to switch a provider or engine to "embral
    // cloud" while signed out. Mounted once by the settings layout (cloud
    // edition only); driven by the shared cloudAuth store.
    import * as Dialog from "$lib/components/ui/dialog";
    import { Button } from "$lib/components/ui/button";
    import { appState } from "$lib/stores/app-state.svelte";
    import { cloudAuth } from "$lib/stores/cloudAuth.svelte";

    function goToAccount() {
        cloudAuth.promptOpen = false;
        appState.openSettings("account");
    }
</script>

<Dialog.Root
    open={cloudAuth.promptOpen}
    onOpenChange={(v) => (cloudAuth.promptOpen = v)}
>
    <Dialog.Content class="sm:max-w-md">
        <Dialog.Header>
            <Dialog.Title>Sign in to use embral cloud</Dialog.Title>
            <Dialog.Description>
                Cloud transcription and summaries run on embral's servers and
                need an account. Sign in to turn them on — everything stays on
                this device until you do.
            </Dialog.Description>
        </Dialog.Header>
        <Dialog.Footer class="gap-2 sm:justify-end">
            <Button
                variant="ghost"
                size="sm"
                onclick={() => (cloudAuth.promptOpen = false)}
            >
                Not now
            </Button>
            <Button size="sm" onclick={goToAccount}>Go to Account</Button>
        </Dialog.Footer>
    </Dialog.Content>
</Dialog.Root>
