import { describe, expect, it } from 'vitest';
import { chipClass } from './speakerColors';

describe('chipClass', () => {
  const labels = ['Speaker 1', 'Speaker 2', 'Speaker 3'];

  it('gives each speaker their own colour', () => {
    const colours = labels.map((l) => chipClass(l, labels));
    expect(new Set(colours).size).toBe(labels.length);
  });

  it('gives one speaker the same colour every time', () => {
    // The whole point: the eye follows a voice down the page.
    expect(chipClass('Speaker 2', labels)).toBe(chipClass('Speaker 2', labels));
  });

  it('colours by position, not by name', () => {
    // A rename must not reshuffle the colours of everyone else.
    const renamed = ['Speaker 1', 'Alice', 'Speaker 3'];
    expect(chipClass('Alice', renamed)).toBe(chipClass('Speaker 2', labels));
  });

  it('falls back rather than throwing on a label it has never seen', () => {
    // A live transcript can render a segment a beat before its label lands in
    // the list; that must not blow up the row.
    expect(chipClass('Nobody', labels)).toBe(chipClass(labels[0], labels));
  });

  it('wraps around once there are more speakers than colours', () => {
    const many = Array.from({ length: 8 }, (_, i) => `Speaker ${i + 1}`);
    expect(chipClass('Speaker 7', many)).toBe(chipClass('Speaker 1', many));
  });
});
