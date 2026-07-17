// The frontend half of the cloud seam (see docs/cloud-seam.md).
//
// This is the ONLY file outside src/lib/cloud/ that may reference that
// directory — and it must do so through `import.meta.glob`, never a
// literal `import('./cloud/…')`: the public-repo filter drops
// src/lib/cloud/ wholesale, and a literal specifier fails svelte-check and
// rollup the moment the directory is gone (found by the first real
// open-core build). The glob resolves to an empty map in that tree, so
// the offline build stays green — the invariant the seam promises.
// Cloud UI renders only when the build was produced with
// VITE_EMBRAL_CLOUD=1 (paired with the Rust `cloud` cargo feature).
import type { Component } from 'svelte';

export const CLOUD_ENABLED = import.meta.env.VITE_EMBRAL_CLOUD === '1';

const cloudComponents = import.meta.glob('./cloud/*.svelte');

async function loadCloudComponent(name: string): Promise<Component<any> | null> {
  if (!CLOUD_ENABLED) return null;
  const load = cloudComponents[`./cloud/${name}.svelte`];
  if (!load) return null;
  const module = (await load()) as { default: Component<any> };
  return module.default;
}

/** Lazily load the Account settings section (cloud builds only). */
export function loadAccountSection() {
  return loadCloudComponent('AccountSection');
}

/** Lazily load the sidebar's hours-remaining ring (cloud builds only). */
export function loadHoursRing() {
  return loadCloudComponent('HoursRing');
}
