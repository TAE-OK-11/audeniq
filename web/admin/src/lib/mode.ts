// 데이터 모드 — 기본 빌드는 실서버(/api/staff/*). `bun run dev:demo` / `build:demo`(VITE_MOCK=true)만 예시 데이터.
export const MOCK = import.meta.env.VITE_MOCK === 'true';
