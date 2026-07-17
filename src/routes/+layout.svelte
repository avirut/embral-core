<script lang="ts">
	import './layout.css';
	import * as Tooltip from '$lib/components/ui/tooltip';

	const { children } = $props();
</script>

<!-- One provider for the whole app. It is not optional: bits-ui's Tooltip.Root
     reads the provider off context and throws without it, which is why the
     vendored primitive sat unused and every hover label in the app was still a
     native Windows tooltip.

     The delay is ours, not the default. The provider defaults to 0 — instant —
     and a rail of icon buttons then fires a tooltip on every mouse traverse. -->
<Tooltip.Provider delayDuration={400}>
	{@render children()}
</Tooltip.Provider>
