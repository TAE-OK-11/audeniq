import { describe, expect, it } from 'vitest';
import { rereleaseIssue, type RereleaseData } from './rerelease';

const tracks = [{ id: 't1', isrc: 'KRABC2600001' }, { id: 't2', isrc: 'KRABC2600002' }];
const base: RereleaseData & { previousTitle: string } = {
  rereleaseKind: 'transfer', previousTitle: '이전 앨범', previousAvailability: 'live', rereleaseAudio: 'same',
  rereleaseRights: 'owned', rereleaseAck: true,
  rereleaseTracks: [{ trackId: 't1', previousIsrc: 'KR-ABC-26-00001' }, { trackId: 't2', previousIsrc: 'KR-ABC-26-00002' }],
};
const issue = (patch: Partial<typeof base>, current = tracks) => rereleaseIssue({ ...base, ...patch }, current, '2025-01-01', '2026-12-01');
describe('이전·재발매 상황 확인', () => {
  it('같은 녹음은 트랙별로 기존 ISRC와 연결한다', () => {
    expect(issue({})).toBeNull();
    expect(issue({}, [...tracks].reverse())).toBeNull();
    expect(issue({}, [{ ...tracks[0], isrc: '' }, tracks[1]])?.field).toBe('aqPreviousIsrc-0');
    expect(issue({ rereleaseTracks: [base.rereleaseTracks![0]] })?.field).toBe('aqPreviousIsrc-1');
  });
  it('새 버전에 기존 녹음 코드를 재사용하지 않는다', () => {
    const patch = { rereleaseKind: 'new_version', rereleaseAudio: 'changed' } as const;
    expect(issue(patch)?.field).toBe('aqPreviousIsrc-0');
    expect(issue(patch, tracks.map(t => ({ ...t, isrc: '' })))).toBeNull();
    expect(issue({ rereleaseAudio: 'changed' })?.field).toBe('aqRereleaseKind');
  });
  it('권한·녹음 상태 확인 전에는 접수를 막고, 서비스 미확인은 사유를 받는다', () => {
    expect(issue({ rereleaseRights: 'pending' })?.field).toBe('aqRereleaseRights');
    expect(issue({ rereleaseAudio: 'unknown' })?.field).toBe('aqRereleaseAudio');
    expect(issue({ previousAvailability: 'unknown' })?.field).toBe('aqRereleaseNotes');
    expect(issue({ previousAvailability: 'unknown', rereleaseNotes: '이전 유통사 확인 대기' })).toBeNull();
    expect(rereleaseIssue(base, tracks, '2027-01-01', '2026-12-01')?.field).toBe('aqOriginalDate');
  });
});
