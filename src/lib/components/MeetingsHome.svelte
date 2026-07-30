<script lang="ts">
  import { onMount } from 'svelte';
  import { Trash2 } from 'lucide-svelte';
  import MeetingDetail from './MeetingDetail.svelte';
  import MeetingList from './MeetingList.svelte';
  import ResizableSplit from './ResizableSplit.svelte';
  import ConfirmDialog from './ConfirmDialog.svelte';
  import { meetingsStore, PENDING_MEETING_ID } from '$lib/stores/meetings.svelte';
  import { copy } from '$lib/copy';

  const t = $derived(copy.meetings);

  type MobilePanel = 'list' | 'detail';

  let mobilePanel = $state<MobilePanel>('list');
  let confirmDelete = $state(false);
  let deleting = $state(false);

  function showDetail() {
    mobilePanel = 'detail';
  }

  function showList() {
    mobilePanel = 'list';
  }

  onMount(() => {
    meetingsStore.load();
  });

  const selection = $derived(meetingsStore.selection);
  /** The pending meeting has no row to delete, so it never counts towards one. */
  const deletableIds = $derived(selection.ids.filter((id) => id !== PENDING_MEETING_ID));
  const multi = $derived(selection.count > 1);

  function onKeydown(e: KeyboardEvent) {
    if (e.key !== 'Delete' || confirmDelete) return;
    // Delete belongs to whatever the user is typing in, if anything.
    const target = e.target as HTMLElement | null;
    if (
      target &&
      (target.tagName === 'INPUT' ||
        target.tagName === 'TEXTAREA' ||
        target.isContentEditable)
    ) {
      return;
    }
    if (deletableIds.length === 0) return;
    e.preventDefault();
    confirmDelete = true;
  }

  async function deleteSelected() {
    deleting = true;
    try {
      await meetingsStore.deleteSelected();
      confirmDelete = false;
      showList();
    } catch {
      // The store keeps the error; the list surfaces it.
    } finally {
      deleting = false;
    }
  }

  const count = $derived(deletableIds.length);
</script>

<svelte:window onkeydown={onKeydown} />

<!-- Wide windows get the draggable list/detail split; narrow ones collapse
     to a single panel (the pane-class overrides below keep one instance of
     each component mounted either way). -->
<div class="flex min-h-0 flex-1 flex-col">
  <ResizableSplit
    fixedSide="left"
    storageKey="embral:meetings-list-width"
    defaultSize={380}
    minFixed={260}
    minFlex={420}
    fixedClass="{mobilePanel === 'detail'
      ? 'hidden'
      : 'flex'} w-full min-[960px]:flex min-[960px]:w-(--split-size) min-[960px]:shrink-0"
    flexClass="{mobilePanel === 'list'
      ? 'hidden'
      : 'flex'} min-w-0 flex-1 min-[960px]:flex"
    dividerClass="hidden min-[960px]:block"
  >
    {#snippet left()}
      <MeetingList onSelect={showDetail} onDelete={() => (confirmDelete = true)} />
    {/snippet}
    {#snippet right()}
      {#if multi}
        <!-- Several meetings picked: there is no single one to read, so the pane
             says what is selected and what can be done with it. -->
        <div class="flex min-h-0 flex-1 flex-col items-center justify-center gap-4 p-8">
          <p class="text-sm text-muted-foreground">
            {t.multiSelect.selected(selection.count)}
          </p>
          <button
            onclick={() => (confirmDelete = true)}
            disabled={count === 0}
            class="inline-flex h-9 items-center gap-2 rounded-md border border-border px-3 text-sm font-medium transition-colors hover:bg-destructive hover:text-white disabled:opacity-50"
          >
            <Trash2 size={15} />
            {t.multiSelect.delete(count)}
          </button>
          <p class="text-xs text-muted-foreground">{t.multiSelect.hint}</p>
        </div>
      {:else}
        <MeetingDetail showBack={mobilePanel === 'detail'} onBack={showList} />
      {/if}
    {/snippet}
  </ResizableSplit>
</div>

<ConfirmDialog
  bind:open={confirmDelete}
  title={t.deleteConfirm.title(count)}
  body={t.deleteConfirm.body(count)}
  confirmLabel={t.deleteConfirm.confirm(count)}
  busy={deleting}
  onConfirm={deleteSelected}
/>
