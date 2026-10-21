// AI 앱 연결 승인 (/ai/connect) — crates/core/src/mcp/connect.rs
// 서버(/api/oauth/authorize·/api/oauth/device)가 서명한 요청을 넘겨주면, 이 화면이 내용을 받아 보여 주고 결정을 보낸다.
import { req } from './http';

export interface AiConnectScope {
  scope: string; title: string; description: string;
  /** 항상 포함 (발매 정보 보기) */
  locked: boolean;
  /** 처음에 켜 둘지 */
  checked: boolean;
  risk: 'none' | 'submit' | 'money';
}

export interface AiConnectDetails {
  kind: 'browser' | 'device';
  client: { name: string; client_id: string; verified_host: string | null; registered: boolean };
  requested_from: string | null;
  return_to: string | null;
  device_code: string | null;
  scopes: AiConnectScope[];
  workspaces: { id: string; name: string; role: 'OWNER' | 'EDITOR' | 'VIEWER' }[];
  expires_at: string;
}

export interface AiConnectResult {
  kind: 'browser' | 'device';
  status: 'APPROVED' | 'DENIED';
  redirect?: string;
  scopes: string[];
}

export const aiConnectDetails = (request: string) =>
  req<AiConnectDetails>(`/api/auth/ai-connect?request=${encodeURIComponent(request)}`);

export const aiConnectByCode = (userCode: string) =>
  req<{ request: string }>('/api/auth/ai-connect/device', { method: 'POST', body: { user_code: userCode } });

export const decideAiConnect = (body: { request: string; decision: 'approve' | 'deny'; org?: string; scopes?: string[] }) =>
  req<AiConnectResult>('/api/auth/ai-connect', { method: 'POST', body });
