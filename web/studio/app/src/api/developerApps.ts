// 개발자 앱 — 내 서비스에서 AUDENIQ API(/api/v1)를 OAuth 2.0으로 쓰는 앱 (crates/core/src/mcp/developer.rs).
// 처음에는 만든 사람만 연결할 수 있고(PRIVATE), 검토를 요청하면 REVIEW, 운영자가 승인하면 누구나 연결(PUBLIC).
import { req } from './http';

export type AppVisibility = 'PRIVATE' | 'REVIEW' | 'PUBLIC';

export interface DeveloperApp {
  id: string;
  client_id: string;
  name: string;
  description: string | null;
  website: string | null;
  redirect_uris: string[];
  /** 모바일·데스크톱 앱: 비밀 키 없이 PKCE로만 */
  public_client: boolean;
  visibility: AppVisibility;
  /** 운영자가 돌려보낸 이유 */
  review_note: string | null;
  review_requested_at: string | null;
  reviewed_at: string | null;
  created_at: string;
  /** 이 앱을 연결한 사람 수 */
  connections: number;
  /** 만들 때와 새 비밀 키를 만들 때만 한 번 */
  client_secret?: string;
}

export interface DeveloperApps {
  items: DeveloperApp[];
  limit: number;
  api_base: string;
  resource: string;
  authorization_endpoint: string;
  token_endpoint: string;
  openapi: string;
}

export interface NewDeveloperApp {
  name: string;
  redirect_uris: string[];
  description?: string;
  website?: string;
  public_client?: boolean;
}

const base = '/api/auth/developer-apps';
const one = (id: string) => `${base}/${encodeURIComponent(id)}`;

export const developerApps = () => req<DeveloperApps>(base);
export const createDeveloperApp = (body: NewDeveloperApp) => req<DeveloperApp>(base, { method: 'POST', body });
export const updateDeveloperApp = (id: string, body: { redirect_uris?: string[]; description?: string; website?: string }) =>
  req<DeveloperApp>(one(id), { method: 'PATCH', body });
export const rotateDeveloperAppSecret = (id: string) => req<DeveloperApp>(`${one(id)}/secret`, { method: 'POST', body: {} });
export const requestDeveloperAppReview = (id: string) => req<DeveloperApp>(`${one(id)}/review`, { method: 'POST', body: {} });
export const deleteDeveloperApp = (id: string) => req<{ deleted: boolean }>(one(id), { method: 'DELETE', body: {} });

export const VISIBILITY_LABEL: Record<AppVisibility, string> = {
  PRIVATE: '나만 연결',
  REVIEW: '검토 중',
  PUBLIC: '공개',
};
