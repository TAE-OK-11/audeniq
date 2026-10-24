// 채널별 링크 — 같은 스마트링크에 ?src=채널 을 붙여 어디서 들어왔는지 센다 (서버 SOURCES와 같은 목록).
export const CHANNELS: { id: string; label: string }[] = [
  { id: 'instagram', label: '인스타그램' },
  { id: 'youtube', label: '유튜브' },
  { id: 'tiktok', label: '틱톡' },
  { id: 'x', label: 'X' },
  { id: 'threads', label: '스레드' },
  { id: 'facebook', label: '페이스북' },
  { id: 'kakao', label: '카카오톡' },
  { id: 'naver', label: '네이버' },
  { id: 'blog', label: '블로그' },
  { id: 'email', label: '이메일' },
  { id: 'qr', label: 'QR 코드' },
  { id: 'poster', label: '포스터·전단' },
];

export const channelLabel = (id: string) => CHANNELS.find(c => c.id === id)?.label ?? id;

/** 채널 표시를 붙인 주소 */
export function channelUrl(url: string, channel: string): string {
  return `${url}${url.includes('?') ? '&' : '?'}src=${encodeURIComponent(channel)}`;
}
