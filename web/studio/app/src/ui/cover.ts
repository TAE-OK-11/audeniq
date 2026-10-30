// 발매 커버 그라데이션 — 같은 발매는 항상 같은 색 (스튜디오·관리자 공통)
// 브랜드 파랑(#3B63F3) 한 색의 밝기 변주만 — 보라·하늘색이 섞이면 흔한 보라→파랑 그라데이션처럼 보였다.
// 가운데 흰 음표 3:1 이상, 짙은 끝(이니셜 아바타가 쓰는 쪽) 흰 글자 4.5:1 이상
const COVER_GRADIENTS = [
  'linear-gradient(135deg,#D6E1FF,#3B63F3 50%,#1F3BB0)',
  'linear-gradient(135deg,#C9D8FF,#2C56E0 55%,#172E8C)',
  'linear-gradient(135deg,#E0E8FF,#4A6FF4 48%,#2442BE)',
  'linear-gradient(135deg,#BFD0FF,#3459E6 55%,#142A7A)',
  'linear-gradient(135deg,#D3DEFF,#4468EE 50%,#23408F)',
  'linear-gradient(135deg,#DDE6FF,#4F72F1 45%,#2A4CC8)',
  'linear-gradient(135deg,#CAD6F5,#3D5BB8 55%,#1E2C66)',
];

/** ID 해시로 커버 그라디언트를 고정 선택 (같은 발매는 항상 같은 색) */
export function gradientFor(id: string): string {
  let h = 0;
  for (let i = 0; i < id.length; i++) h = (h * 31 + id.charCodeAt(i)) >>> 0;
  return COVER_GRADIENTS[h % COVER_GRADIENTS.length];
}

