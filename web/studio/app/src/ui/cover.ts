// 발매 커버 그라데이션 — 같은 발매는 항상 같은 색 (스튜디오·관리자 공통)
// AUDENIQ 파랑·보라 계열만 — 초록·주황 같은 다른 색이 섞이면 화면이 산만해 보인다
const COVER_GRADIENTS = [
  'linear-gradient(135deg,#C9DAFF,#5B7CF5 62%,#2C3F9E)',
  'linear-gradient(135deg,#DCD3FF,#8B6CF0 62%,#4A3497)',
  'linear-gradient(135deg,#BFE3FF,#4C93EE 62%,#23508F)',
  'linear-gradient(135deg,#D6DEFF,#6E79E8 62%,#33378C)',
  'linear-gradient(135deg,#E6D8FF,#A57BEA 62%,#5A3B8E)',
  'linear-gradient(135deg,#C4ECFA,#3FA2D9 62%,#1F5680)',
  'linear-gradient(135deg,#D0D8EE,#6F7FA8 62%,#36415F)',
];

/** ID 해시로 커버 그라디언트를 고정 선택 (같은 발매는 항상 같은 색) */
export function gradientFor(id: string): string {
  let h = 0;
  for (let i = 0; i < id.length; i++) h = (h * 31 + id.charCodeAt(i)) >>> 0;
  return COVER_GRADIENTS[h % COVER_GRADIENTS.length];
}

