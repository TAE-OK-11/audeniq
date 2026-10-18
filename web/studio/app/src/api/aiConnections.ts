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
  /** CONSENT: AI 앱에서 브라우저로 승인 · DEVICE: 링크와 코드로 승인 · PASSWORD: 등록된 에이전트에 로그인 · TOKEN: 연결 토큰 */
  method?: 'CONSENT' | 'DEVICE' | 'PASSWORD' | 'TOKEN';
  /** 연결 토큰에 붙인 이름 */
  label?: string | null;
  /** 연결 토큰의 만료 시각 (무기한이면 null) */
  expires_at?: string | null;
  /** 연결 토큰이 무기한인지 */
  no_expiry?: boolean;
}

export interface NewConnectionToken {
  id: string;
  /** 한 번만 보여 준다. 서버에는 해시만 남는다. */
  token: string;
  /** 무기한이면 null */
  expires_at: string | null;
  no_expiry: boolean;
  scopes: string[];
  mcp_url: string;
}

export const aiConnections = () => req<{ items: AiConnection[] }>('/api/auth/ai-connections').then(r => r.items);
export const revokeAiConnection = (id: string) =>
  req<{ revoked: boolean }>(`/api/auth/ai-connections/${encodeURIComponent(id)}/revoke`, { method: 'POST', body: {} });

/** OAuth를 쓰지 못하는 에이전트용 고정 토큰 — 지금 작업 공간에서, 고른 권한만. */
export const createConnectionToken = (input: { label: string; org_id: string; scopes: string[]; days?: number; no_expiry?: boolean }) =>
  req<NewConnectionToken>('/api/auth/ai-connections/tokens', { method: 'POST', body: input });

/** 동의 화면과 같은 이름 */
export const SCOPE_LABEL: Record<string, string> = {
  'releases:read': '발매 정보 보기',
  'releases:write': '발매 초안 작성·수정',
  'releases:submit': '발매 신청 접수',
  'settlement:read': '정산 보기',
  'payouts:request': '소액 지급 신청',
};

/** 권한마다 한 줄 설명 (동의 화면과 같은 뜻) */
export const SCOPE_HELP: Record<string, string> = {
  'releases:read': '발매·트랙·검사 결과·접수 상태를 읽어요.',
  'releases:write': '임시 저장 상태의 발매 정보·트랙·크레딧을 만들고 고쳐요.',
  'releases:submit': '대화에서 확인한 내용 그대로 배급 신청을 접수해요.',
  'settlement:read': '잔액·정산 내역·지급 현황을 읽어요. 계좌번호는 보이지 않아요.',
  'payouts:request': '등록한 계좌로 한도 안의 소액 지급을 신청해요.',
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

/** AI 앱으로 한 발매 신청 중 스튜디오 승인을 기다리는 것 (설정에서 켠 경우) */
export interface AiSubmission {
  id: string;
  org_id: string;
  org_name: string;
  release_id: string;
  title: string;
  client_name: string;
  declarations: Record<string, boolean>;
  created_at: string;
  expires_at: string;
}

/** 스튜디오 승인 설정과, 서버가 지금 허용하는 것 (MCP가 꺼져 있으면 연결 토큰을 만들 수 없어요) */
export interface AiSettings {
  studio_approval: boolean;
  mcp_enabled?: boolean;
  /** 소액 지급 신청 권한을 줄 수 있는지 (AI 지급 한도가 0이면 false) */
  payout_scope?: boolean;
}
export const aiSettings = () => req<AiSettings>('/api/auth/ai-settings');
export const saveAiSettings = (studio_approval: boolean) =>
  req<{ studio_approval: boolean }>('/api/auth/ai-settings', { method: 'PUT', body: { studio_approval } });
export const aiSubmissions = () => req<{ items: AiSubmission[] }>('/api/auth/ai-submissions').then(r => r.items);
export const approveAiSubmission = (id: string) =>
  req<{ status: string }>(`/api/auth/ai-submissions/${encodeURIComponent(id)}/approve`, { method: 'POST', body: {} });
export const rejectAiSubmission = (id: string) =>
  req<{ status: string }>(`/api/auth/ai-submissions/${encodeURIComponent(id)}/reject`, { method: 'POST', body: {} });

/** 발매 신청 때 아티스트가 하는 선언 (내 이름으로 기록돼요) */
export const DECLARATION_LABEL: Record<string, string> = {
  rights_confirmed: '음원·작품·커버에 필요한 권리를 갖고 있어요',
  adult_confirmed: '만 18세 이상이에요',
  is_cover: '다른 사람의 곡을 커버했어요',
  is_remix: '기존 음원의 리믹스예요',
  contains_samples: '다른 음원을 샘플링했어요',
  ai_involved: '음악·보컬·가사에 AI 도구를 썼어요',
  explicit_content: '청소년에게 유해한 내용이 있어요',
};
