// Cloudflare Worker에서 쓰는 만큼만 적은 타입 — @cloudflare/workers-types를 들이지 않고 필요한 모양만.

/** 정적 에셋·서비스 바인딩 (env.ASSETS) */
export interface Fetcher {
  fetch(input: Request | string | URL, init?: RequestInit): Promise<Response>;
}

/** D1 (설문 저장) */
export interface D1Result<T = Record<string, unknown>> {
  results?: T[];
  success: boolean;
  meta: { changes?: number; [key: string]: unknown };
}
export interface D1PreparedStatement {
  bind(...values: unknown[]): D1PreparedStatement;
  first<T = Record<string, unknown>>(column?: string): Promise<T | null>;
  run<T = Record<string, unknown>>(): Promise<D1Result<T>>;
  all<T = Record<string, unknown>>(): Promise<D1Result<T>>;
}
export interface D1Database {
  prepare(query: string): D1PreparedStatement;
}

export interface ExecutionContext {
  waitUntil(promise: Promise<unknown>): void;
  passThroughOnException(): void;
}
export interface ScheduledController {
  scheduledTime: number;
  cron: string;
}

/** shared/transport.ts가 읽는 Worker 환경 — 각 Worker의 Env는 이걸 넓혀 쓴다 */
export interface TransportEnv {
  ASSETS: Fetcher;
  /** 백엔드 출처 (기본 https://api-origin.audeniq.com) */
  BACKEND_URL?: string;
  /** 엣지 → 백엔드 서비스 신원 비밀 */
  EDGE_SERVICE_SECRET?: string;
}
