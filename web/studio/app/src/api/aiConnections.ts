// 연결된 AI 앱 — ChatGPT·Claude 같은 AI 어시스턴트가 AUDENIQ MCP로 이 계정에 연결한 내역.
// 서버 crates/core/src/mcp/oauth.rs (connections, revoke_connection). 끊으면 그 앱의 토큰이 바로 무효가 된다.
import { req } from './http';

export interface AiConnection {
  id: string;
  client_name: string;
  /** DCR이면 임의 ID, CIMD면 앱이 공개한 https 주소 */
  client_id: string;
  org_id: string;
  org_name: string;
  scopes: string[];
  created_at: string;
  last_used_at: string | null;
}

export const aiConnections = () => req<{ items: AiConnection[] }>('/api/auth/ai-connections').then(r => r.items);
export const revokeAiConnection = (id: string) =>
  req<{ revoked: boolean }>(`/api/auth/ai-connections/${encodeURIComponent(id)}/revoke`, { method: 'POST', body: {} });

/** 동의 화면과 같은 이름 */
export const SCOPE_LABEL: Record<string, string> = {
  'releases:read': '발매 정보 보기',
  'releases:write': '발매 초안 작성·수정',
  'releases:submit': '발매 신청 접수',
  'settlement:read': '정산 보기',
  'payouts:request': '소액 지급 신청',
};

/** 앱 확인용 주소의 호스트 (CIMD). 임의 ID면 없음. */
export function clientHost(clientId: string): string | null {
  if (!clientId.startsWith('https://')) return null;
  try {
    return new URL(clientId).host;
  } catch {
    return null;
  }
}
