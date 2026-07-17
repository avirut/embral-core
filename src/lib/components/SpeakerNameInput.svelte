<script lang="ts">
  /** Inline speaker-name editor with an app-themed suggestion popover —
   * replaces the native datalist (which renders as the browser's autofill
   * dropdown). Suggestions come from the profile registry plus the
   * meeting's other labels (for merges); arrows + Enter pick, Escape
   * cancels, blur commits. */

  let {
    value = $bindable(''),
    suggestions = [],
    onCommit,
    onCancel
  }: {
    value?: string;
    suggestions?: string[];
    onCommit: () => void;
    onCancel: () => void;
  } = $props();

  let inputEl: HTMLInputElement | undefined = $state();
  let highlighted = $state(-1);

  const matches = $derived.by(() => {
    const q = value.trim().toLowerCase();
    const seen = new Set<string>();
    const out: string[] = [];
    for (const name of suggestions) {
      const lower = name.toLowerCase();
      if (seen.has(lower) || lower === q) continue;
      if (!q || lower.includes(q)) {
        seen.add(lower);
        out.push(name);
      }
      if (out.length >= 6) break;
    }
    return out;
  });

  $effect(() => {
    inputEl?.focus();
    inputEl?.select();
  });

  function pick(name: string) {
    value = name;
    onCommit();
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      highlighted = Math.min(highlighted + 1, matches.length - 1);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      highlighted = Math.max(highlighted - 1, -1);
    } else if (e.key === 'Enter') {
      e.preventDefault();
      if (highlighted >= 0 && matches[highlighted]) {
        pick(matches[highlighted]);
      } else {
        onCommit();
      }
    } else if (e.key === 'Escape') {
      onCancel();
    } else {
      highlighted = -1;
    }
  }
</script>

<span class="relative inline-block">
  <input
    bind:this={inputEl}
    bind:value
    class="h-6 w-36 rounded-full border border-input bg-background px-2.5 text-[11px] outline-none focus:ring-1 focus:ring-ring"
    onkeydown={onKeydown}
    onblur={onCommit}
    aria-label="Speaker name"
  />
  {#if matches.length > 0}
    <div
      class="absolute top-full left-1 z-50 mt-1 min-w-36 overflow-hidden rounded-md border border-border bg-popover py-1 shadow-md"
    >
      {#each matches as name, i (name)}
        <!-- pointerdown beats the input's blur, so picking works. -->
        <button
          class="block w-full truncate px-2.5 py-1 text-left text-xs transition-colors {i ===
          highlighted
            ? 'bg-accent text-accent-foreground'
            : 'text-popover-foreground hover:bg-accent hover:text-accent-foreground'}"
          onpointerdown={(e) => {
            e.preventDefault();
            pick(name);
          }}
        >
          {name}
        </button>
      {/each}
    </div>
  {/if}
</span>
