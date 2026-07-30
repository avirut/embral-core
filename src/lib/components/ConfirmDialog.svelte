<script lang="ts">
  /**
   * The app's destructive-action confirm. This markup was duplicated verbatim
   * in the meeting delete and the settings reset; it lives here now so a
   * "delete 3 meetings?" looks exactly like a "reset the app?".
   *
   * Deliberately not the bits-ui dialog: this is a two-button question, not a
   * modal surface, and the vendored dialog brings a scroll lock the shell does
   * not want.
   */
  import { copy } from '$lib/copy';

  const t = $derived(copy.common);

  let {
    open = $bindable(false),
    title,
    body,
    confirmLabel = t.delete,
    busy = false,
    onConfirm
  }: {
    open?: boolean;
    title: string;
    body: string;
    confirmLabel?: string;
    busy?: boolean;
    onConfirm: () => void;
  } = $props();

  function onKeydown(e: KeyboardEvent) {
    if (!open) return;
    if (e.key === 'Escape') {
      e.preventDefault();
      open = false;
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#if open}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/35 px-4">
    <div class="w-full max-w-sm rounded-md border border-border bg-background p-4 shadow-lg">
      <h3 class="text-sm font-semibold">{title}</h3>
      <p class="mt-2 text-sm text-muted-foreground">{body}</p>
      <div class="mt-4 flex justify-end gap-2">
        <button
          onclick={() => (open = false)}
          class="h-9 rounded-md border border-border px-3 text-sm font-medium transition-colors hover:bg-accent"
          disabled={busy}
        >
          {t.cancel}
        </button>
        <button
          onclick={onConfirm}
          class="h-9 rounded-md bg-destructive px-3 text-sm font-medium text-white transition-colors hover:bg-destructive/90 disabled:opacity-50"
          disabled={busy}
        >
          {confirmLabel}
        </button>
      </div>
    </div>
  </div>
{/if}
