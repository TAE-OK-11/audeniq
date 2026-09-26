import { describe, expect, it } from 'vitest';
import { normalizeRelease } from './mock';

describe('normalizeRelease', () => {
  it('구버전·손상 데이터를 안전하게 보정', () => {
    expect(normalizeRelease(null)).toBeNull();
    expect(normalizeRelease({ title: 'no id' })).toBeNull();
    const r = normalizeRelease({ id: 'x', status: 'weird', tracks: [{ id: 't' }, null, 'bad'], draft: { history: 'nope' } })!;
    expect(r.status).toBe('draft');
    expect(r.tracks).toHaveLength(1);
    expect(r.track_count).toBe(1);
    expect(r.title).toBe('제목 없는 발매');
    expect(r.draft?.history).toEqual([]);
    expect(r.draft?.platforms).toEqual([]);
  });
});
