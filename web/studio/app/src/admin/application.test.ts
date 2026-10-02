// 심사 화면의 원본 확인이 스튜디오가 서명할 때 계산한 문서 확인 코드와 같은 규칙인지 확인한다.
import { describe, expect, it } from 'vitest';
import { createApplication } from '../lib/application';
import { sanitizeProfile } from '../api/remote';
import type { ReleasePayload } from '../api/types';
import { verifyDraft, type StudioDraft } from './application';

const payload: ReleasePayload = {
  title: '새벽의 온도', artist: '한결', type: 'single', language: 'ko', genre: 'Indie Pop', genreCustom: '',
  label: '한결 뮤직', upc: '', notes: '첫 싱글이에요.', coverName: 'cover.jpg', coverData: '', originalDate: '',
  release_date: '2026-10-20', territories: ['WORLD'], platforms: ['melon', 'spotify'],
  ownership: '한결 뮤직', phonogram: '2026 한결 뮤직', copyright: '2026 한결 뮤직',
  rightsChecks: { rightsMaster: true },
  options: {
    express: true, expressAck: true, expressReason: '공연 일정에 맞춰야 해요', minor: false,
    guardian: '', guardianRelation: '', guardianContact: '', guardian2: '', guardian2Relation: '', guardian2Contact: '',
    guardianConsentDone: false, familyCertName: '', familyCertMethod: '',
    cover: false, coverTracks: [], coverRightsAck: false, coverLicenseFile: '',
    sample: false, sampleLicenseFile: '', featured: false, featuredConsentFile: '',
    ai: true, aiTool: '', aiUses: ['편곡·반주에 AI를 사용했어요'], shared: false, sharedContractFile: '',
    rerelease: false, previousTitle: '', previousId: '',
  },
  artistProfile: { isNew: true, spotify: '', apple: '', melon: '' },
  tracks: [{
    id: 't1', title: '새벽의 온도', version: '', isrc: '', composers: '한결', lyricists: '한결', arrangers: '',
    performers: '', producer: '', lyrics: '창밖에\n새벽', audioName: '01.wav', audioSize: 1, explicit: false,
    duration: '03:12', audioSpec: 'WAV · 24bit · 48kHz · 스테레오',
  }],
};

/** 스튜디오가 서버에 저장하는 모양 (api/remote.ts buildProfile의 키) */
function savedDraft(p: ReleasePayload): StudioDraft {
  return sanitizeProfile({
    artist: p.artist, type: p.type, language: p.language, genre: p.genre, genreCustom: p.genreCustom, label: p.label,
    upc: p.upc, notes_lines: p.notes.split('\n'), originalDate: p.originalDate, release_date: p.release_date,
    territories: p.territories, platforms: p.platforms, ownership: p.ownership, phonogram: p.phonogram,
    copyright: p.copyright, rightsChecks: p.rightsChecks, options: p.options, draftTracks: p.tracks,
    artistProfile: p.artistProfile, application: p.application,
  }) as StudioDraft;
}

describe('심사 화면 신청서 원본 확인', () => {
  it('서명한 내용 그대로면 일치, 서명 후 크레딧이 바뀌면 불일치', async () => {
    const app = await createApplication({ payload, signerName: '한결', signerRole: '아티스트 본인', signature: 'data:image/png;base64,AA', agreements: ['truth', 'terms', 'privacy', 'esign'] });
    const draft = savedDraft({ ...payload, application: app });
    expect(await verifyDraft(draft, payload.title, app.hash)).toBe('ok');

    const edited = savedDraft({ ...payload, tracks: [{ ...payload.tracks[0], composers: '다른 사람' }], application: app });
    expect(await verifyDraft(edited, payload.title, app.hash)).toBe('changed');
    // 서버에 기록된 코드와 다르면 불일치
    expect(await verifyDraft(draft, payload.title, 'f'.repeat(64))).toBe('changed');
  });

  it('서명 기록이 없으면 unsigned', async () => {
    expect(await verifyDraft(savedDraft(payload), payload.title)).toBe('unsigned');
  });
});
