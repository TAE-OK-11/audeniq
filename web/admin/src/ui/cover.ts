// 발매 커버 그라데이션 — 같은 발매는 항상 같은 색 (스튜디오·관리자 공통)
const COVER_GRADIENTS = [
  'linear-gradient(135deg,#bcd8ff,#7162db 65%,#394b83)',
  'linear-gradient(135deg,#ffd9c1,#e58a7a 65%,#8a3d4b)',
  'linear-gradient(135deg,#bdf0d9,#4dae8a 65%,#2a6b52)',
  'linear-gradient(135deg,#ecd9ff,#a97ae5 65%,#5b3d8a)',
  'linear-gradient(135deg,#fff0b8,#e0a83f 65%,#8a6a2a)',
  'linear-gradient(135deg,#b8e6ff,#5c9ae5 65%,#2a4d8a)',
  'linear-gradient(135deg,#ffcfe3,#e57aa5 65%,#8a3d63)',
];

/** ID 해시로 커버 그라디언트를 고정 선택 (같은 발매는 항상 같은 색) */
export function gradientFor(id: string): string {
  let h = 0;
  for (let i = 0; i < id.length; i++) h = (h * 31 + id.charCodeAt(i)) >>> 0;
  return COVER_GRADIENTS[h % COVER_GRADIENTS.length];
}

