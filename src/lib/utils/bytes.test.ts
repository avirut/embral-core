import { describe, expect, it } from 'vitest';
import { formatBytes } from './bytes';

describe('formatBytes', () => {
  it('speaks MB below a gigabyte and GB from one up', () => {
    expect(formatBytes(45_200_000)).toBe('45 MB');
    expect(formatBytes(662_000_000)).toBe('662 MB');
    expect(formatBytes(2_500_000_000)).toBe('2.5 GB');
    expect(formatBytes(1_000_000_000)).toBe('1.0 GB');
  });

  it('is empty for nothing to say', () => {
    expect(formatBytes(0)).toBe('');
    expect(formatBytes(undefined)).toBe('');
    expect(formatBytes(null)).toBe('');
  });
});
