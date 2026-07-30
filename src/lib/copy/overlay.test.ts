// The platform overlay: merge semantics, and the macOS overlay's fit
// against the live catalog (docs/copy.md).

import { describe, expect, it } from 'vitest';
import { en } from './en';
import { mac } from './en/mac';
import { overlay } from './overlay';

describe('overlay', () => {
  it('replaces leaves and keeps untouched siblings identical', () => {
    const base = { a: { b: 'base-b', c: 'base-c' }, d: 'base-d' };
    const merged = overlay(base, { a: { b: 'over-b' } });
    expect(merged.a.b).toBe('over-b');
    expect(merged.a.c).toBe('base-c');
    // Untouched subtrees keep referential identity — no wasteful copies.
    expect(merged.d).toBe(base.d);
    // The base is never mutated.
    expect(base.a.b).toBe('base-b');
  });

  it('applies the macOS overlay onto the catalog', () => {
    const merged = overlay(en, mac);
    expect(merged.shell.titleBar.commandBar.shortcut).toBe('⌘K');
    expect(merged.settings.general.appearance.indicator.accent).toBe('System accent');
    expect(merged.meetings.recording.star).toContain('⌘S');
    expect(merged.settings.mcp.missingServer).not.toContain('.exe');
    // A sibling the overlay never names is untouched.
    expect(merged.shell.titleBar.minimize).toBe(en.shell.titleBar.minimize);
    expect(merged.settings.general.appearance.indicator.colors).toBe(
      en.settings.general.appearance.indicator.colors
    );
  });

  it('overlay strings are corpus-clean', () => {
    // The corpus test walks the shipped `copy` (Windows-shaped under the
    // test runner), so the overlay's own strings get the same value-shape
    // floor here: non-empty, trimmed, no stray braces.
    const walk = (node: unknown, path: string): void => {
      if (typeof node === 'string') {
        expect(node, `${path} is empty`).not.toBe('');
        expect(node, `${path} has edge whitespace`).toBe(node.trim());
        expect(node, `${path} contains a stray brace`).not.toMatch(/[{}]/);
        return;
      }
      if (typeof node === 'object' && node !== null) {
        for (const [k, v] of Object.entries(node)) walk(v, `${path}.${k}`);
      }
    };
    walk(mac, 'mac');
  });
});
