import { describe, expect, it, vi } from 'vitest';

// The real one needs a Tauri webview; the shape is all these tests need.
vi.mock('@tauri-apps/api/core', () => ({
  convertFileSrc: (path: string) => `http://asset.localhost/${encodeURIComponent(path)}`
}));

const { toDisplaySrc, toStoredSrc } = await import('./assetSrc');

const ROOT = 'C:\\Users\\avirut\\embral';
const REL = 'assets/260326T143000_a3f9b2/img-01.png';

describe('toDisplaySrc', () => {
  it('resolves a storage-relative link against the library root', () => {
    const out = toDisplaySrc(ROOT, REL);
    expect(out).toContain('asset.localhost');
    expect(decodeURIComponent(out)).toContain('embral\\assets\\260326T143000_a3f9b2\\img-01.png');
  });

  it('is idempotent, so a copied image survives being pasted again', () => {
    const once = toDisplaySrc(ROOT, REL);
    expect(toDisplaySrc(ROOT, once)).toBe(once);
  });

  it('leaves remote and inline images alone', () => {
    for (const foreign of ['https://x.test/i.png', 'data:image/png;base64,AAAA']) {
      expect(toDisplaySrc(ROOT, foreign)).toBe(foreign);
    }
  });

  it('passes the link through rather than mangling it with no root', () => {
    expect(toDisplaySrc('', REL)).toBe(REL);
  });
});

describe('toStoredSrc', () => {
  it('recovers the storage-relative link from a display src', () => {
    expect(toStoredSrc(toDisplaySrc(ROOT, REL))).toBe(REL);
  });

  it('is idempotent', () => {
    expect(toStoredSrc(REL)).toBe(REL);
    expect(toStoredSrc(toStoredSrc(toDisplaySrc(ROOT, REL)))).toBe(REL);
  });

  it('handles a library on another drive', () => {
    const other = toDisplaySrc('D:\\meetings', REL);
    expect(toStoredSrc(other)).toBe(REL);
  });

  it('leaves remote and inline images alone', () => {
    for (const foreign of ['https://x.test/i.png', 'data:image/png;base64,AAAA']) {
      expect(toStoredSrc(foreign)).toBe(foreign);
    }
  });

  it('round-trips through a POSIX library path too', () => {
    const posix = toDisplaySrc('/Users/avirut/embral', REL);
    expect(toStoredSrc(posix)).toBe(REL);
  });
});
