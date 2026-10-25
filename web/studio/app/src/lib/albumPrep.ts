// 앨범별 관리 — 발매일까지 남은 날(D-day)과 발매 준비 현황(무엇이 끝났고 무엇이 남았는지).
// 화면(ReleaseDetail·Releases)은 이 결과만 그린다. 서버 값으로만 판단하고, 모르는 것은 '기다림'으로 둔다(지어내지 않는다).
import type { DraftTrack, Release, ReleaseDraft, Track } from '../api/types';

const DAY = 86_400_000;
const dayOf = (d: Date) => Date.UTC(d.getFullYear(), d.getMonth(), d.getDate());

/** 발매일까지 남은 날 — 'D-12' · '오늘 발매' · '발매 3일째'(발매 후 30일까지). 끝났거나 날짜가 없으면 null */
export function releaseDay(date: string | null | undefined, status: string, now = new Date()): { text: string; soon: boolean } | null {
  if (!date || ['cancelled', 'rejected', 'closed'].includes(status)) return null;
  const [y, m, d] = date.split('-').map(Number);
  if (!y || !m || !d) return null;
  const n = Math.round((Date.UTC(y, m - 1, d) - dayOf(now)) / DAY);
  if (n > 0) return status === 'live' ? null : { text: `D-${n}`, soon: n <= 7 };
  if (n === 0) return { text: '오늘 발매', soon: true };
  return status === 'live' && n >= -29 ? { text: `발매 ${1 - n}일째`, soon: false } : null;
}

export type PrepState = 'done' | 'todo' | 'wait';
export interface PrepItem {
  key: 'cover' | 'audio' | 'credits' | 'date' | 'agreement' | 'proofs' | 'codes';
  label: string;
  /** 지금 상태를 한 줄로 */
  sub: string;
  state: PrepState;
}

/** 서버 트랙과 신청서의 곡 정보를 맞춘다 (서버 트랙 ID = 신청서 곡의 serverId 또는 id) */
export function draftTrackOf(t: Track, draft?: ReleaseDraft | null): DraftTrack | undefined {
  return draft?.draftTracks?.find(x => x.serverId === t.id || x.id === t.id);
}

export interface PrepInput {
  rel: Pick<Release, 'status' | 'release_date'> & { tracks: Track[] };
  draft?: ReleaseDraft | null;
  coverReady: boolean;
  /** 배급 계약서: 없음(검토 뒤 열림) · 서명 필요 · 서명함 */
  agreement: 'none' | 'to-sign' | 'signed';
  /** 담당자가 요청했는데 아직 내지 않은 권리 서류 수 */
  proofsDue: number;
}

/** 발매 준비 현황 — 아티스트가 할 일(todo), 끝난 일(done), AUDENIQ이 할 일을 기다리는 것(wait) */
export function albumPrep({ rel, draft, coverReady, agreement, proofsDue }: PrepInput): PrepItem[] {
  const tracks = rel.tracks;
  const n = tracks.length;
  const withAudio = tracks.filter(t => !!t.audioName).length;
  const credited = tracks.filter(t => {
    const dt = draftTrackOf(t, draft);
    const lyricsOk = !!(t.lyricists ?? dt?.lyricists)?.trim() || !!dt?.instrumental;
    return !!(t.composers ?? dt?.composers)?.trim() && lyricsOk;
  }).length;
  const isrcs = tracks.filter(t => !!t.isrc).length;
  const upc = !!draft?.upc?.trim();
  const items: PrepItem[] = [
    { key: 'cover', label: '커버아트', state: coverReady ? 'done' : 'todo', sub: coverReady ? '올렸어요' : '3000×3000 정사각형 이미지를 올려 주세요' },
    {
      key: 'audio', label: '음원 파일', state: n > 0 && withAudio === n ? 'done' : 'todo',
      sub: n === 0 ? '곡을 등록해 주세요' : withAudio === n ? `${n}곡 모두 올렸어요` : `${n}곡 중 ${n - withAudio}곡이 남았어요`,
    },
    {
      key: 'credits', label: '크레딧', state: n > 0 && credited === n ? 'done' : 'todo',
      sub: n === 0 ? '곡을 등록해 주세요' : credited === n ? '작곡·작사를 모두 적었어요' : `${n - credited}곡의 작곡·작사가 비어 있어요`,
    },
    { key: 'date', label: '발매일', state: rel.release_date ? 'done' : 'todo', sub: rel.release_date ?? '발매일을 정해 주세요' },
    {
      key: 'agreement', label: '배급 계약서',
      state: agreement === 'signed' ? 'done' : agreement === 'to-sign' ? 'todo' : 'wait',
      sub: agreement === 'signed' ? '서명했어요' : agreement === 'to-sign' ? '검토가 끝났어요. 서명해 주세요' : '검토가 끝나면 서명할 수 있어요',
    },
    { key: 'proofs', label: '권리 서류', state: proofsDue > 0 ? 'todo' : 'done', sub: proofsDue > 0 ? `요청받은 서류 ${proofsDue}건을 내 주세요` : '요청받은 서류가 없어요' },
    {
      key: 'codes', label: 'UPC·ISRC', state: upc && n > 0 && isrcs === n ? 'done' : 'wait',
      sub: upc && n > 0 && isrcs === n ? '모두 발급됐어요' : '승인되면 AUDENIQ이 발급해요',
    },
  ];
  return items;
}
