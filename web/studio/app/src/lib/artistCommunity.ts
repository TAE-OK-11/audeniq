// 아티스트 페이지 소식·팬 응원·디스코그래피 — 공개 페이지와 스튜디오 관리 화면이 같이 쓴다 (서버: artist_community.rs).
export type PostKind = 'NOTICE' | 'EVENT';
export interface ArtistPost {
  id: string; kind: PostKind; title: string; body: string;
  starts_at: string | null; place: string; url: string; pinned: boolean;
  created_at: string; updated_at: string;
}
export interface Cheer { id: string; nickname: string; message: string; heart: boolean; hidden?: boolean; created_at: string }
export interface Community { posts: ArtistPost[]; posts_total: number; cheers_enabled: boolean; cheers_total: number }
export interface DiscoTrack { title: string; disc: number; number: number; artist: string | null }

export const POST_TITLE_MAX = 80;
export const POST_BODY_MAX = 2000;
export const POST_PLACE_MAX = 80;
export const NICK_MAX = 20;
export const CHEER_MAX = 300;
export const KIND_LABEL: Record<PostKind, string> = { NOTICE: '공지', EVENT: '이벤트' };

const TZ = 'Asia/Seoul';
const parts = (d: Date) => Object.fromEntries(
  new Intl.DateTimeFormat('ko-KR', { timeZone: TZ, year: 'numeric', month: 'numeric', day: 'numeric', weekday: 'short', hour: 'numeric', minute: '2-digit', hour12: true })
    .formatToParts(d).map(p => [p.type, p.value]),
) as Record<string, string>;

/** '2026년 5월 1일 (금) 오후 7:00' — 올해면 연도를 뺀다. 공연은 한국 시간으로 */
export function eventWhen(iso: string, now = new Date()): string {
  const d = new Date(iso);
  const p = parts(d);
  const year = p.year !== parts(now).year ? `${p.year}년 ` : '';
  return `${year}${p.month}월 ${p.day}일 (${p.weekday}) ${p.dayPeriod} ${p.hour}:${p.minute}`;
}

/** 한국 날짜 기준 남은 날 — 0이면 오늘, 음수면 지남 */
export function daysUntil(iso: string, now = new Date()): number {
  const day = (d: Date) => {
    const p = parts(d);
    return Date.UTC(Number(p.year), Number(p.month) - 1, Number(p.day));
  };
  return Math.round((day(new Date(iso)) - day(now)) / 86_400_000);
}

/** 'D-12' · '오늘' · '지난 이벤트' */
export function dday(iso: string, now = new Date()): { text: string; past: boolean } {
  const n = daysUntil(iso, now);
  if (n < 0) return { text: '지난 이벤트', past: true };
  return { text: n === 0 ? '오늘' : `D-${n}`, past: false };
}

/** '방금' · '5분 전' · '3시간 전' · '2일 전' · '2026.3.14' */
export function ago(iso: string, now = new Date()): string {
  const s = Math.max(0, (now.getTime() - new Date(iso).getTime()) / 1000);
  if (s < 60) return '방금';
  if (s < 3600) return `${Math.floor(s / 60)}분 전`;
  if (s < 86400) return `${Math.floor(s / 3600)}시간 전`;
  if (s < 86400 * 7) return `${Math.floor(s / 86400)}일 전`;
  const p = parts(new Date(iso));
  return `${p.year}.${p.month}.${p.day}`;
}

/** <input type="datetime-local"> 값(한국 시간) ↔ ISO */
export function localToIso(v: string): string | null {
  return /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}$/.test(v) ? new Date(`${v}:00+09:00`).toISOString() : null;
}
export function isoToLocal(iso: string | null): string {
  if (!iso) return '';
  const k = new Date(new Date(iso).getTime() + 9 * 3600_000).toISOString();
  return k.slice(0, 16);
}

// 체험 모드 데이터 — 공개 페이지와 관리 화면이 같은 걸 보여 준다
const day = 86_400_000;
const at = (d: number, h = 19) => {
  const t = new Date(Date.now() + d * day);
  return new Date(`${t.toISOString().slice(0, 10)}T${String(h).padStart(2, '0')}:00:00+09:00`).toISOString();
};
export const MOCK_POSTS: ArtistPost[] = [
  { id: 'p1', kind: 'NOTICE', title: '새 EP ‘겨울 산책’ 작업을 마쳤어요', body: '많이 기다려 주셔서 고마워요.\n발매 전에 미리 저장해 두면 나오는 날 바로 들을 수 있어요.', starts_at: null, place: '', url: '', pinned: true, created_at: at(-2), updated_at: at(-2) },
  { id: 'p2', kind: 'EVENT', title: '단독 공연 ‘새벽의 도시’', body: '처음으로 하는 단독 공연이에요. 새 노래도 들려 드릴게요.', starts_at: at(18), place: '홍대 롤링홀', url: 'https://tickets.example.com/hangyeol', pinned: false, created_at: at(-5), updated_at: at(-5) },
  { id: 'p3', kind: 'NOTICE', title: '인스타그램 라이브 다시 보기를 올렸어요', body: '', starts_at: null, place: '', url: 'https://www.instagram.com/hangyeol', pinned: false, created_at: at(-9), updated_at: at(-9) },
];
export const MOCK_CHEERS: Cheer[] = [
  { id: 'c1', nickname: '새벽산책', message: '‘여름의 끝’ 매일 듣고 있어요. 공연 꼭 갈게요!', heart: true, created_at: new Date(Date.now() - 40 * 60_000).toISOString() },
  { id: 'c2', nickname: '한결팬1호', message: '첫 싱글부터 응원했어요 💙', heart: false, created_at: new Date(Date.now() - 5 * 3600_000).toISOString() },
  { id: 'c3', nickname: '도시의밤', message: '가사가 너무 좋아요. 위로가 됐어요.', heart: false, created_at: new Date(Date.now() - 3 * day).toISOString() },
];
