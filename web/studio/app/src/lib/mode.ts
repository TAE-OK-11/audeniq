// 데이터 모드 — `bun run build:edge`(또는 VITE_MOCK=false)면 실제 서버, 아니면 브라우저 저장소 체험 모드.
// 스토어가 api/client를 import하면 순환 참조가 생기므로 모드 판정만 따로 둔다.
export const MOCK = import.meta.env.MODE !== 'edge' && import.meta.env.VITE_MOCK !== 'false';
