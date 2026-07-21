import { describe, expect, it } from 'vitest';
import {
  ASR_ACCURATE,
  ASR_BALANCED,
  ASR_FAST,
  EMBEDDING,
  LLM_RUNTIME,
  LLM_WEIGHTS,
  PUNCTUATION,
  SPEAKER_ID,
  diskWarning,
  missingBytes,
  recommend,
  recommendedIds,
  recommendedLanguage,
  type SystemSpecs
} from './recommend';
import type { ModelStatus } from '$lib/types';

const GB = 1024 * 1024 * 1024;

function specs(cores: number, ramGb: number, freeGb = 0): SystemSpecs {
  return {
    logical_cores: cores,
    total_ram_bytes: ramGb * GB,
    free_disk_bytes: freeGb * GB
  };
}

describe('recommend', () => {
  it('gives 16 GB machines the accurate model without punctuation', () => {
    const rec = recommend(specs(12, 16));
    expect(rec.asr).toBe(ASR_ACCURATE);
    expect(rec.punct).toBe(false);
    expect(rec.llm).toBe(true);
  });

  it('ignores cores entirely — ram is the only gate', () => {
    // The 8-core/32 GB laptop that motivated the change, and a low-core
    // machine with plenty of RAM.
    expect(recommend(specs(8, 32)).asr).toBe(ASR_ACCURATE);
    expect(recommend(specs(4, 16)).asr).toBe(ASR_ACCURATE);
    // Many cores never compensate for missing RAM.
    expect(recommend(specs(24, 8)).asr).toBe(ASR_BALANCED);
    expect(recommend(specs(2, 8)).asr).toBe(ASR_BALANCED);
  });

  it('pairs punctuation with the zipformers', () => {
    expect(recommend(specs(4, 8)).punct).toBe(true);
    expect(recommend(specs(4, 32)).punct).toBe(false);
  });

  it('sends weak machines to the small model, no llm', () => {
    const rec = recommend(specs(4, 4));
    expect(rec.asr).toBe(ASR_FAST);
    expect(rec.punct).toBe(true);
    expect(rec.llm).toBe(false);
  });

  it('gates the llm on ram alone', () => {
    expect(recommend(specs(4, 16)).llm).toBe(true);
    expect(recommend(specs(24, 8)).llm).toBe(false);
  });
});

describe('recommendedLanguage', () => {
  it('reads english from the tag', () => {
    expect(recommendedLanguage('en-US')).toBe('english');
    expect(recommendedLanguage('EN')).toBe('english');
    expect(recommendedLanguage('en-GB')).toBe('english');
  });

  it('sends other languages to multilingual', () => {
    expect(recommendedLanguage('de-DE')).toBe('multilingual');
    expect(recommendedLanguage('fr')).toBe('multilingual');
    // "eng"-prefixed lookalikes that are not English tags.
    expect(recommendedLanguage('eo')).toBe('multilingual');
  });

  it('keeps the shipped default when the tag is empty', () => {
    expect(recommendedLanguage('')).toBe('english');
    expect(recommendedLanguage('  ')).toBe('english');
  });
});

describe('recommendedIds', () => {
  it('always carries speaker-id and semantic search', () => {
    for (const s of [specs(4, 4), specs(16, 32)]) {
      const ids = recommendedIds(recommend(s));
      expect(ids).toContain(SPEAKER_ID);
      expect(ids).toContain(EMBEDDING);
    }
  });

  it('pairs the llm runtime with its weights', () => {
    const ids = recommendedIds(recommend(specs(12, 16)));
    expect(ids).toContain(LLM_RUNTIME);
    expect(ids).toContain(LLM_WEIGHTS);
    const weak = recommendedIds(recommend(specs(4, 8)));
    expect(weak).not.toContain(LLM_RUNTIME);
    expect(weak).not.toContain(LLM_WEIGHTS);
    expect(weak).toContain(PUNCTUATION);
  });
});

describe('missingBytes / diskWarning', () => {
  const statuses = [
    { id: 'a', present: false, total_bytes: 3 * GB },
    { id: 'b', present: true, total_bytes: 5 * GB },
    { id: 'c', present: false, total_bytes: 1 * GB }
  ] as ModelStatus[];

  it('sums only absent models', () => {
    expect(missingBytes(['a', 'b', 'c'], statuses)).toBe(4 * GB);
    expect(missingBytes(['b'], statuses)).toBe(0);
    expect(missingBytes(['unknown'], statuses)).toBe(0);
  });

  it('warns only when known free space is short of size plus headroom', () => {
    expect(diskWarning(specs(8, 16, 4), 3 * GB)).toBe(true);
    expect(diskWarning(specs(8, 16, 6), 3 * GB)).toBe(false);
    // Unknown disk never warns.
    expect(diskWarning(specs(8, 16, 0), 100 * GB)).toBe(false);
  });
});
