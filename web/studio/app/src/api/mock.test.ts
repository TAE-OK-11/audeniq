import { beforeEach, describe, expect, it } from 'vitest';
import type { ReleasePayload } from './client';
import { mockApi } from './mock';

const payload = (title: string): ReleasePayload => ({
  title, artist: 'A', type: 'single', language: 'ko', genre: 'Pop', genreCustom: '', label: '',
  upc: '', notes: '', coverName: '', coverData: '', originalDate: '', release_date: '2030-01-01',
  tracks: [{ id: 't', title: 'x', version: '', isrc: '', composers: 'c', lyricists: '', arrangers: '', performers: '', producer: '', lyrics: '', audioName: 'a.wav', audioSize: 1, explicit: false, duration: '03:10' }],
  territories: ['WORLD'], platforms: ['spotify'], ownership: 'A', phonogram: 'p', copyright: 'c',
  rightsChecks: {}, options: {} as ReleasePayload['options'],
});

describe('mockApi releases', () => {
  beforeEach(() => mockApi.resetData());

  it('임시 저장 후 접수하면 발매가 하나만 생긴다 (중복 draft 버그 회귀 방지)', async () => {
    const before = (await mockApi.listReleases()).length;
    const d = await mockApi.saveDraft(null, payload('새 곡'));
    await mockApi.saveDraft(d.id, payload('새 곡 수정'));
    const r = await mockApi.submitRelease(d.id, payload('새 곡 최종'));
    const list = await mockApi.listReleases();
    expect(list.length).toBe(before + 1);
    expect(r.id).toBe(d.id);
    expect(r.status).toBe('review');
    const detail = await mockApi.getRelease(r.id);
    expect(detail.tracks[0].duration_ms).toBe(190000);
    expect(detail.draft?.history.at(-1)?.text).toContain('접수');
  });

  it('접수된 발매는 임시 저장으로 상태가 되돌아가지 않는다', async () => {
    const r = await mockApi.saveDraft('r2', payload('여름 EP'));
    expect(r.status).toBe('review');
  });

  it('삭제', async () => {
    await mockApi.deleteRelease('r4');
    await expect(mockApi.getRelease('r4')).rejects.toThrow();
  });

  it('로그인 검증', async () => {
    await expect(mockApi.login('bad', 'x')).rejects.toThrow();
    await expect(mockApi.signup('a@b.co', 'short')).rejects.toThrow();
    const u = await mockApi.login('a@b.co', 'pw');
    expect((await mockApi.me()).email).toBe(u.email);
    await mockApi.logout();
    await expect(mockApi.me()).rejects.toThrow();
  });
});
