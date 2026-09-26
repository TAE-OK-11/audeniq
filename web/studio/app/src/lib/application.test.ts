import { describe, expect, it } from 'vitest';
import type { ReleaseDetail, ReleasePayload } from '../api/types';
import { createApplication, displayCode, hashLabel, legacyApplication, verifyApplication } from './application';

const payload: ReleasePayload = {
  title: '밤의 정원', artist: '서린', type: 'single', language: 'ko', genre: 'Pop', genreCustom: '', label: '',
  upc: '', notes: '', coverName: 'c.png', coverData: '', originalDate: '', release_date: '2030-01-01',
  tracks: [{ id: 't1', title: '밤의 정원', version: '', isrc: '', composers: '서린', lyricists: '서린', arrangers: '', performers: '', producer: '', lyrics: '가사', audioName: 'a.wav', audioSize: 1, explicit: false, duration: '03:05', featuring: '하온', instrumental: false, audioSpec: 'WAV · 24bit · 48kHz · 스테레오' }],
  territories: ['WORLD'], platforms: ['spotify'], ownership: '서린', phonogram: '2030 서린', copyright: '2030 서린',
  rightsChecks: {}, options: { express: false, cover: true } as ReleasePayload['options'],
  artistProfile: { isNew: true, spotify: '', apple: '', melon: '' },
};

// 저장 후 불러온 발매 (mock·서버 모두 draft에 신청 내용이 남는다)
const saved = (p: ReleasePayload): ReleaseDetail => ({
  id: 'r1', title: p.title, status: 'review', release_date: p.release_date, created_at: '2030-01-01', track_count: 1,
  artist: p.artist, tracks: [],
  draft: { ...p, history: [], draftTracks: p.tracks.map(t => ({ ...t, serverId: 'srv-1' })) },
});

describe('배급 신청서', () => {
  it('서명한 내용으로 번호·해시를 만들고, 저장된 발매와 대조한다', async () => {
    const app = await createApplication({ payload, signerName: ' 서린 ', signerRole: '아티스트 본인', signature: 'data:image/png;base64,AA', agreements: ['truth'] });
    expect(app.no).toMatch(/^AUD-\d{8}-[A-Z2-9]{6}$/);
    expect(app.form).toMatch(/^AUD-/);
    expect(app.hash).toMatch(/^[0-9a-f]{64}$/);
    expect(app.signerName).toBe('서린');
    expect(hashLabel(app.hash).split(' ')).toHaveLength(8);
    expect(await verifyApplication(app, saved(payload))).toBe(true);

    // 접수 후 곡 정보·서명이 바뀌면 불일치
    const changed = saved({ ...payload, tracks: [{ ...payload.tracks[0], composers: '다른 사람' }] });
    expect(await verifyApplication(app, changed)).toBe(false);
    expect(await verifyApplication({ ...app, signature: 'data:image/png;base64,BB' }, saved(payload))).toBe(false);
  });

  it('예전 접수 건도 같은 번호로 신청서를 만들고, AQ 번호는 AUD로 표기한다', async () => {
    const rel: ReleaseDetail = {
      id: 'r1', title: '첫 번째 싱글', status: 'live', release_date: '2026-10-01', created_at: '2026-09-20', track_count: 1, artist: '서린',
      tracks: [{ id: 't1', title: '첫 번째 싱글', duration_ms: 214000, isrc: 'KRA262600001', composers: '서린' }],
      draft: { type: 'single', genre: 'Pop', label: '', upc: '', notes: '', coverName: '', territories: ['WORLD'], platforms: [], ownership: '서린', phonogram: '', copyright: '', rightsChecks: {}, history: [{ text: '발매 신청 접수 완료', time: '2026-09-20 14:32' }] },
    };
    const a = await legacyApplication(rel);
    const b = await legacyApplication(rel);
    expect(a.no).toBe(b.no);
    expect(a.no).toMatch(/^AUD-20260920-[A-Z2-9]{6}$/);
    expect(a.submittedAt).toBe('2026-09-20 14:32');
    expect(a.signerName).toBe('서린');
    expect(displayCode('AQ-20260926-ABCDEF')).toBe('AUD-20260926-ABCDEF');
    expect(displayCode('AQ-DIST-APP 1.0')).toBe('AUD-DIST-APP 1.0');
  });
});
