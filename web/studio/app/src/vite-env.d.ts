/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** 'false'면 실제 API 호출, 그 외에는 브라우저 저장소 기반 목 데이터 */
  readonly VITE_MOCK?: string;
  /** 실제 API 기본 주소 (예: https://api.audeniq.com). 비우면 같은 출처 */
  readonly VITE_API_BASE?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
