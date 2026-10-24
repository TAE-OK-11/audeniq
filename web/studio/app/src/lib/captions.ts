// 홍보 문구 — 발매 예고·발매 당일·발매 후 SNS에 바로 붙여 넣을 글. 스마트링크(채널 표시 포함)와 해시태그까지.
// 아티스트가 고쳐 쓰는 출발점이라 짧고 담백하게.
import { daysBetween } from './home';
import { todayStr } from './date';

export type CaptionStage = 'teaser' | 'release' | 'after';
export const CAPTION_STAGES: { value: CaptionStage; label: string }[] = [
  { value: 'teaser', label: '발매 예고' },
  { value: 'release', label: '발매 당일' },
  { value: 'after', label: '발매 후' },
];

export interface CaptionInput {
  title: string; artist: string; releaseDate: string | null; releaseTime?: string | null; url: string;
  /** 발매 형태 (싱글·EP·정규) */
  kind?: string;
}

/** 지금 쓰기 좋은 단계 — 발매 전이면 예고, 당일이면 당일, 지나면 발매 후 */
export function defaultStage(releaseDate: string | null, today = todayStr()): CaptionStage {
  if (!releaseDate) return 'after';
  const d = daysBetween(today, releaseDate);
  return d > 0 ? 'teaser' : d === 0 ? 'release' : 'after';
}

/** 해시태그 한 개 — 공백·기호를 빼고 (없으면 빈 문자열) */
export function hashtag(s: string): string {
  const t = s.normalize('NFC').replace(/[^\p{L}\p{N}_]/gu, '');
  return t ? `#${t}` : '';
}

export function hashtags(i: CaptionInput): string {
  const tags = [hashtag(i.artist), hashtag(i.title), '#신곡', '#NewMusic'];
  return [...new Set(tags.filter(Boolean))].join(' ');
}

const dateText = (date: string, time?: string | null) =>
  `${Number(date.slice(5, 7))}월 ${Number(date.slice(8, 10))}일${time && /^\d{2}:\d{2}$/.test(time) ? ` ${time}` : ''}`;

export function caption(i: CaptionInput, stage: CaptionStage, today = todayStr()): string {
  // 제목에 이미 들어 있으면('여름 EP') 형태를 또 붙이지 않는다
  const kind = i.kind && !i.title.toLowerCase().includes(i.kind.toLowerCase()) ? `${i.kind} ` : '';
  const name = `${i.artist ? `${i.artist} ` : ''}${kind}‘${i.title}’`;
  let body: string[];
  if (stage === 'teaser' && i.releaseDate) {
    const d = daysBetween(today, i.releaseDate);
    body = [
      `${name} ${dateText(i.releaseDate, i.releaseTime)} 발매${d > 0 ? ` (D-${d})` : ''}`,
      '',
      '발매되면 바로 들을 수 있게 미리 저장하고 알림 받아 두세요.',
    ];
  } else if (stage === 'release') {
    body = [`${name} 오늘 발매됐어요.`, '', '지금 좋아하는 플랫폼에서 들어 주세요.'];
  } else {
    body = [`${name}`, '', '아직 못 들어 보셨다면 지금 들어 주세요.'];
  }
  return [...body, `🎧 ${i.url}`, '', hashtags(i)].join('\n');
}
