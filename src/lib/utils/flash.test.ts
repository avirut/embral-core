// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { FLASH_CLASS, FLASH_MS, flash } from './flash';

describe('flash', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  function el(): HTMLElement {
    const node = document.createElement('p');
    document.body.append(node);
    return node;
  }

  it('marks the element and clears itself', () => {
    const node = el();
    flash(node);
    expect(node.classList.contains(FLASH_CLASS)).toBe(true);
    vi.advanceTimersByTime(FLASH_MS + 1);
    expect(node.classList.contains(FLASH_CLASS)).toBe(false);
  });

  it('restarts rather than being cut short by the first run', () => {
    const node = el();
    flash(node);
    vi.advanceTimersByTime(FLASH_MS - 100);
    flash(node);
    // The first run's timer would have fired here; the second must survive.
    vi.advanceTimersByTime(200);
    expect(node.classList.contains(FLASH_CLASS)).toBe(true);
    vi.advanceTimersByTime(FLASH_MS);
    expect(node.classList.contains(FLASH_CLASS)).toBe(false);
  });

  it('is a no-op on nothing, so callers need no guard', () => {
    expect(() => flash(null)).not.toThrow();
    expect(() => flash(undefined)).not.toThrow();
  });
});
