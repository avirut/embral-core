// Apply a per-platform wording overlay onto the catalog (docs/copy.md).
// Objects merge recursively; strings, functions, and arrays replace whole.
// Pure and locale-agnostic: a second locale's macOS overlay goes through
// the same function.

import type { Overlay } from './types';

function isPlainObject(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v);
}

export function overlay<T>(base: T, over: NoInfer<Overlay<T>>): T {
  if (!isPlainObject(base) || !isPlainObject(over)) {
    // Leaf: the overlay value wins outright.
    return over as unknown as T;
  }
  const merged: Record<string, unknown> = { ...base };
  for (const [key, value] of Object.entries(over)) {
    if (value === undefined) continue;
    merged[key] = overlay(base[key], value as Overlay<unknown>);
  }
  return merged as T;
}
