import { describe, expect, it } from 'vitest';
import { albumPrep, releaseDay } from './albumPrep';
import type { Track } from '../api/types';

const now = new Date(2026, 9, 25, 15, 0);
const track = (p: Partial<Track> = {}): Track => ({ id: 't1', title: '파도', duration_ms: 1000, isrc: null, composers: '서린', lyricists: '서린', audioName: 'a.wav', ...p });

describe('releaseDay', () => {
  it('counts down to the release and up for a month after', () => {
    expect(releaseDay('2026-11-15', 'review', now)).toEqual({ text: 'D-21', soon: false });
    expect(releaseDay('2026-10-30', 'scheduled', now)).toEqual({ text: 'D-5', soon: true });
    expect(releaseDay('2026-10-25', 'scheduled', now)).toEqual({ text: '오늘 발매', soon: true });
    expect(releaseDay('2026-10-23', 'live', now)?.text).toBe('발매 3일째');
    expect(releaseDay('2026-08-01', 'live', now)).toBeNull();
  });
  it('says nothing for closed, undated or overdue unreleased', () => {
    expect(releaseDay(null, 'draft', now)).toBeNull();
    expect(releaseDay('2026-11-15', 'cancelled', now)).toBeNull();
    expect(releaseDay('2026-10-01', 'review', now)).toBeNull();
  });
});

describe('albumPrep', () => {
  const base = { rel: { status: 'review', release_date: '2026-11-15', tracks: [track(), track({ id: 't2', audioName: null, lyricists: '' })] }, draft: null, coverReady: true, agreement: 'none' as const, proofsDue: 0 };
  it('separates what the artist does from what AUDENIQ does', () => {
    const by = Object.fromEntries(albumPrep(base).map(i => [i.key, i]));
    expect(by.cover.state).toBe('done');
    expect(by.audio).toMatchObject({ state: 'todo', sub: '2곡 중 1곡이 남았어요' });
    expect(by.credits).toMatchObject({ state: 'todo', sub: '1곡의 작곡·작사가 비어 있어요' });
    expect(by.agreement.state).toBe('wait');
    expect(by.codes.state).toBe('wait');
  });
  it('accepts an instrumental without lyricists and finished codes', () => {
    const items = albumPrep({
      ...base, agreement: 'signed',
      rel: { ...base.rel, tracks: [track({ isrc: 'KR-A1B-26-00001', lyricists: '' })] },
      draft: { upc: '8800000000001', draftTracks: [{ id: 'd1', serverId: 't1', instrumental: true }] } as never,
    });
    expect(items.every(i => i.state === 'done')).toBe(true);
  });
});

describe('empty draft preparation counts', () => {
  it('accounts for seven items: one done, four tasks, two waiting', () => {
    const items = albumPrep({ rel: { status: 'draft', release_date: null, tracks: [] }, coverReady: false, agreement: 'none', proofsDue: 0 });
    expect(items).toHaveLength(7);
    expect(items.filter(i => i.state === 'done')).toHaveLength(1);
    expect(items.filter(i => i.state === 'todo')).toHaveLength(4);
    expect(items.filter(i => i.state === 'wait')).toHaveLength(2);
    expect(items.find(i => i.key === 'audio')?.sub).toContain('음원 파일');
    expect(items.find(i => i.key === 'credits')?.sub).toContain('작곡·작사');
  });
});
