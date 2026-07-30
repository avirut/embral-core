<script lang="ts">
	import { ContextMenu as ContextMenuPrimitive } from "bits-ui";
	import { cn, type WithoutChild, type WithoutChildrenOrChild } from "$lib/utils.js";

	let {
		ref = $bindable(null),
		class: className,
		portalProps,
		// The shell never scrolls behind a popover, and the lock swallows
		// wheel events inside long lists (same fix as the select and dialog).
		preventScroll = false,
		...restProps
	}: WithoutChild<ContextMenuPrimitive.ContentProps> & {
		portalProps?: WithoutChildrenOrChild<ContextMenuPrimitive.PortalProps>;
	} = $props();
</script>

<ContextMenuPrimitive.Portal {...portalProps}>
	<ContextMenuPrimitive.Content
		bind:ref
		{preventScroll}
		data-slot="context-menu-content"
		class={cn(
			"bg-popover text-popover-foreground data-open:animate-in data-closed:animate-out data-closed:fade-out-0 data-open:fade-in-0 data-closed:zoom-out-95 data-open:zoom-in-95 ring-foreground/10 relative isolate z-50 min-w-36 overflow-x-hidden overflow-y-auto rounded-lg p-1 shadow-md ring-1 duration-100 outline-hidden",
			className
		)}
		{...restProps}
	/>
</ContextMenuPrimitive.Portal>
