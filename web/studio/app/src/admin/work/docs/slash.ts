// '/' 명령 목록 — 블록 넣기와 AUDENIQ 블록(실시간 값) 넣기. 실행은 편집기(DocEditor)가 하고,
// 여기에는 이름·별칭·묶음과 거르기만 둔다(키보드만으로 고르기, 단위 테스트).
import type { EmbedKind, EmbedView, EntityKind } from '../types';

export type SlashAction =
  | { kind: 'block'; block: 'text' | 'h1' | 'h2' | 'h3' | 'todo' | 'list' | 'ordered' | 'quote' | 'callout' | 'divider' | 'toggle' | 'code' | 'table' }
  | { kind: 'upload'; as: 'image' | 'file' }
  | { kind: 'link' }
  /** 고르기 창에서 원천을 찾은 뒤 블록을 넣는다 */
  | { kind: 'embed'; pick: EntityKind; embed: EmbedKind; view?: EmbedView }
  | { kind: 'mentionDoc' };

export interface SlashItem { id: string; title: string; hint: string; group: '기본 블록' | '미디어' | 'AUDENIQ'; aliases: string[]; icon: string; action: SlashAction }

export const SLASH_ITEMS: SlashItem[] = [
  { id: 'text', title: '본문', hint: '일반 문단', group: '기본 블록', aliases: ['text', 'p', '텍스트', '문단'], icon: '¶', action: { kind: 'block', block: 'text' } },
  { id: 'h1', title: '제목 1', hint: '큰 제목', group: '기본 블록', aliases: ['h1', 'heading', '제목'], icon: 'H1', action: { kind: 'block', block: 'h1' } },
  { id: 'h2', title: '제목 2', hint: '중간 제목', group: '기본 블록', aliases: ['h2', 'heading', '제목'], icon: 'H2', action: { kind: 'block', block: 'h2' } },
  { id: 'h3', title: '제목 3', hint: '작은 제목', group: '기본 블록', aliases: ['h3', 'heading', '제목'], icon: 'H3', action: { kind: 'block', block: 'h3' } },
  { id: 'todo', title: '할 일 목록', hint: '체크 상자', group: '기본 블록', aliases: ['todo', 'task', 'check', '체크', '할일'], icon: '☑', action: { kind: 'block', block: 'todo' } },
  { id: 'list', title: '글머리 목록', hint: '• 항목', group: '기본 블록', aliases: ['list', 'bullet', 'ul', '목록'], icon: '•', action: { kind: 'block', block: 'list' } },
  { id: 'ordered', title: '번호 목록', hint: '1. 항목', group: '기본 블록', aliases: ['ol', 'number', 'ordered', '번호'], icon: '1.', action: { kind: 'block', block: 'ordered' } },
  { id: 'quote', title: '인용', hint: '인용문', group: '기본 블록', aliases: ['quote', 'blockquote', '인용'], icon: '❝', action: { kind: 'block', block: 'quote' } },
  { id: 'callout', title: '강조 상자', hint: '안내·주의 상자', group: '기본 블록', aliases: ['callout', 'note', 'info', '콜아웃', '강조'], icon: '!', action: { kind: 'block', block: 'callout' } },
  { id: 'divider', title: '구분선', hint: '가로줄', group: '기본 블록', aliases: ['divider', 'hr', 'line', '구분'], icon: '—', action: { kind: 'block', block: 'divider' } },
  { id: 'toggle', title: '접기 블록', hint: '펼치고 접는 내용', group: '기본 블록', aliases: ['toggle', 'details', '토글', '접기'], icon: '▸', action: { kind: 'block', block: 'toggle' } },
  { id: 'code', title: '코드', hint: '코드 블록', group: '기본 블록', aliases: ['code', 'pre', '코드'], icon: '</>', action: { kind: 'block', block: 'code' } },
  { id: 'table', title: '표', hint: '3×3 표', group: '기본 블록', aliases: ['table', 'grid', '표'], icon: '▦', action: { kind: 'block', block: 'table' } },
  { id: 'image', title: '이미지', hint: 'PNG·JPG·WebP 올리기', group: '미디어', aliases: ['image', 'img', 'picture', '이미지', '사진'], icon: '🖼', action: { kind: 'upload', as: 'image' } },
  { id: 'file', title: '파일', hint: '파일 첨부', group: '미디어', aliases: ['file', 'attach', '파일', '첨부'], icon: '📎', action: { kind: 'upload', as: 'file' } },
  { id: 'link', title: '링크 미리보기', hint: '주소와 설명 카드', group: '미디어', aliases: ['link', 'url', 'bookmark', '링크'], icon: '🔗', action: { kind: 'link' } },
  { id: 'doc', title: '다른 문서', hint: '문서 링크', group: '미디어', aliases: ['doc', 'page', '문서'], icon: '📄', action: { kind: 'mentionDoc' } },
  { id: 'release', title: '발매', hint: '이름·아티스트·UPC·발매일·상태', group: 'AUDENIQ', aliases: ['release', '발매', '앨범'], icon: '💿', action: { kind: 'embed', pick: 'release', embed: 'release' } },
  { id: 'album', title: '앨범', hint: '발매 카드 (앨범)', group: 'AUDENIQ', aliases: ['album', '앨범'], icon: '💿', action: { kind: 'embed', pick: 'release', embed: 'release' } },
  { id: 'qc', title: 'QC 결과', hint: '라우드니스·트루 피크·샘플레이트·오류', group: 'AUDENIQ', aliases: ['qc', 'loudness', '품질', '검사'], icon: '✓', action: { kind: 'embed', pick: 'release', embed: 'release', view: 'qc' } },
  { id: 'rights', title: '권리 검토', hint: '계약서·2차 승인·열린 검사', group: 'AUDENIQ', aliases: ['rights', '권리'], icon: '⚖', action: { kind: 'embed', pick: 'release', embed: 'release', view: 'rights' } },
  { id: 'distribution', title: '배급 현황', hint: 'DSP별 전송·라이브 상태', group: 'AUDENIQ', aliases: ['distribution', 'delivery', '배급', '전송'], icon: '🚚', action: { kind: 'embed', pick: 'release', embed: 'release', view: 'distribution' } },
  { id: 'track', title: '트랙', hint: 'ISRC·오디오 사양', group: 'AUDENIQ', aliases: ['track', '트랙', '음원'], icon: '♪', action: { kind: 'embed', pick: 'track', embed: 'track' } },
  { id: 'artist', title: '아티스트', hint: '아티스트 카드', group: 'AUDENIQ', aliases: ['artist', '아티스트'], icon: '★', action: { kind: 'embed', pick: 'artist', embed: 'artist' } },
  { id: 'dsp', title: 'DSP', hint: '전송·실패·마지막 오류', group: 'AUDENIQ', aliases: ['dsp', 'store', '플랫폼'], icon: '🌐', action: { kind: 'embed', pick: 'dsp', embed: 'dsp' } },
  { id: 'ticket', title: '고객 문의', hint: '번호·제목·상태·담당', group: 'AUDENIQ', aliases: ['ticket', 'inquiry', 'support', '문의', '티켓'], icon: '✉', action: { kind: 'embed', pick: 'inquiry', embed: 'inquiry' } },
  { id: 'kanban', title: '칸반 카드', hint: '제목·우선순위·열·담당·마감', group: 'AUDENIQ', aliases: ['kanban', 'card', 'task', '카드', '업무'], icon: '▤', action: { kind: 'embed', pick: 'task', embed: 'task' } },
  { id: 'approval', title: '2차 승인', hint: '승인 요청 상태', group: 'AUDENIQ', aliases: ['approval', '승인'], icon: '✔', action: { kind: 'embed', pick: 'approval', embed: 'approval' } },
  { id: 'audit', title: '감사 기록', hint: '발매의 변경 기록 (심사 권한)', group: 'AUDENIQ', aliases: ['audit', 'log', '감사', '기록'], icon: '🧾', action: { kind: 'embed', pick: 'release', embed: 'audit' } },
];

/** 입력한 글자로 거르기 — 이름·별칭 앞부분이 맞는 것을 먼저 */
export function filterSlash(query: string, items: SlashItem[] = SLASH_ITEMS): SlashItem[] {
  const q = query.trim().toLowerCase();
  if (!q) return items;
  const score = (it: SlashItem) => {
    const names = [it.id, it.title.toLowerCase(), ...it.aliases.map(a => a.toLowerCase())];
    if (names.some(n => n === q)) return 0;
    if (names.some(n => n.startsWith(q))) return 1;
    if (names.some(n => n.includes(q))) return 2;
    return 9;
  };
  return items.map(it => [it, score(it)] as const).filter(([, s]) => s < 9).sort((a, b) => a[1] - b[1]).map(([it]) => it);
}
