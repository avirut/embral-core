<script lang="ts">
    // Shown when the user tries to switch a provider or engine to "embral
    // cloud" while signed out. Mounted once by the settings layout (cloud
    // edition only); driven by the shared cloudAuth store.
    import * as Dialog from "$lib/components/ui/dialog";
    import { Button } from "$lib/components/ui/button";
    import { appState } from "$lib/stores/app-state.svelte";
    import { cloudAuth } from "$lib/stores/cloudAuth.svelte";
    import { copy } from "$lib/copy";

    const t = $derived(copy.settings.cloudSignIn);

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
            <Dialog.Title>{t.title}</Dialog.Title>
            <Dialog.Description>
                {t.description}
            </Dialog.Description>
        </Dialog.Header>
        <Dialog.Footer class="gap-2 sm:justify-end">
            <Button
                variant="ghost"
                size="sm"
                onclick={() => (cloudAuth.promptOpen = false)}
            >
                {t.notNow}
            </Button>
            <Button size="sm" onclick={goToAccount}>{t.goToAccount}</Button>
        </Dialog.Footer>
    </Dialog.Content>
</Dialog.Root>
