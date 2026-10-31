// 운영 도구 — 예전에는 audeniq-admin CLI에만 있던 관리 작업 (백엔드 `/api/staff/ops/*`, crates/core/src/staff_ops.rs).
// 레벨: 고객 지원 1 < 심사 담당·배급 운영 2 < 관리자 3 < 최고 관리자 4. 돈은 레벨이 아니라 정산 업무(배급 운영·관리자·최고 관리자).
// 직원 권한·돈·식별코드 범위·파트너 라이브·AI 에이전트 등록은 인증 앱 코드를 한 번 더 받는다(step_up).
import { req } from '../api/http';
import { MOCK } from '../lib/mode';

export type OpsArea = 'staff' | 'protected' | 'developer-apps' | 'ai-clients' | 'ai-limits' | 'partners' | 'identifiers' | 'payouts' | 'royalty' | 'balance' | 'publishing';

export interface OpsEntry { area: OpsArea; op: string; write: boolean; step_up: boolean; allowed: boolean; level: number; finance: boolean }
export interface OpsIndex { available: boolean; role: string; level: number; ops: OpsEntry[] }

const qs = (q: Record<string, string | undefined>) => {
  const p = new URLSearchParams();
  for (const [k, v] of Object.entries(q)) if (v) p.set(k, v);
  const s = p.toString();
  return s ? `?${s}` : '';
};

const remote = {
  index: () => req<OpsIndex>('/api/staff/ops', { quietServer: true }),
  read: <T>(area: OpsArea, op: string, q: Record<string, string | undefined> = {}) =>
    req<T>(`/api/staff/ops/${area}/${op}${qs(q)}`, { quietServer: true }),
  write: <T>(area: OpsArea, op: string, body: Record<string, unknown>) =>
    req<T>(`/api/staff/ops/${area}/${op}`, { method: 'POST', body, quietServer: true }),
};
type OpsApi = typeof remote;

/** 체험 모드에서만 예시 데이터 (opsMock.ts는 실서버 빌드에 들어가지 않는다) */
// 실서버 빌드에서 바로 거짓이 되는 식을 앞에 둔다 — 다른 모듈의 상수(MOCK)만으로는 실서버 빌드에도 예시 데이터 청크가 만들어졌다
export const opsApi: OpsApi = import.meta.env.MODE !== 'edge' && import.meta.env.VITE_MOCK !== 'false' && MOCK
  ? {
    index: () => import('./opsMock').then(m => m.mockOps.index()),
    read: <T>(area: OpsArea, op: string, q: Record<string, string | undefined> = {}) => import('./opsMock').then(m => m.mockOps.read<T>(area, op, q)),
    write: <T>(area: OpsArea, op: string, body: Record<string, unknown>) => import('./opsMock').then(m => m.mockOps.write<T>(area, op, body)),
  }
  : remote;

export const entry = (index: OpsIndex | undefined, area: OpsArea, op: string) =>
  index?.ops.find(o => o.area === area && o.op === op);
export const can = (index: OpsIndex | undefined, area: OpsArea, op: string) => !!index?.available && !!entry(index, area, op)?.allowed;

// 목록 모양 (서버 응답 그대로)
export interface StaffRow { email: string; user_id: string; role: string; status: 'ACTIVE' | 'REVOKED'; granted_by: string; granted_at: string; revoked_at: string | null }
export interface ProtectedRow {
  id: string; name: string; active: boolean; match_mode: 'CONTAINS' | 'TOKEN'; action: 'BLOCK' | 'REVIEW'; note: string | null;
  aliases: { alias: string; kind: 'NAME' | 'PHRASE'; match_mode: string; action: string }[];
  exceptions: { org_id: string; reason: string; granted_by: string; revoked: boolean }[];
}
export interface DevAppRow {
  client_id: string; name: string; owner_user_id: string; owner_email?: string | null; visibility: 'PRIVATE' | 'REVIEW' | 'PUBLIC';
  website: string | null; description: string | null; redirect_uris: string[];
  review_requested_at: string | null; reviewed_at: string | null; review_note: string | null; created_at: string;
}
export interface AgentRow { client_id: string; name: string; password_grant: boolean; auth: string; redirect_uris: string[]; created_at: string; disabled_at: string | null }
export interface LimitRow { user_id: string; email?: string | null; daily_limit: number; note: string | null; set_by: string; updated_at: string }
export interface PartnerRow { partner_id: string; display_name: string; dsp_id: string | null; transport: string; activation_kind: string; delivery_enabled: boolean }
export interface PartnerStatus {
  partner_id: string; stage: string; dpid_registered: boolean; endpoint_url: string | null; endpoint_health: string;
  credential_kind: string | null; credential_status: string; test_ern_validated_at: string | null; test_ack_parsed_at: string | null;
  contract_signed_at: string | null; contract_ref: string | null; gaps: string[];
}
export interface IssuerRow { kind: 'UPC' | 'ISRC'; mode: string; prefix: string; active: boolean; created_at: string; issued: Record<string, number> }
export interface PayoutRow {
  request_id: string; org_id: string; amount: string; fee: string; net: string; status: string;
  account: { bank_name?: string; bank_code?: string; account_masked?: string; holder_name?: string; [k: string]: unknown };
  created_at: string; order_id: string | null; order_status: string | null;
}
export interface RoyaltyReport { report_id: string; dsp_id: string; status: string; currency: string; source_hash: string; source_amount: string; received_amount: string; unresolved: number }
/** 정산금 직접 추가 (최고 관리자만) — 계정의 작업 공간마다 지금 받을 수 있는 금액과 최근 추가 기록 */
export interface BalanceCredit { id: string; title: string; reason: string | null; amount: string; created_at: string }
export interface BalanceOrg { org_id: string; name: string; kind: string; role: string; withdrawable: string; credits: BalanceCredit[] }
export interface BalanceAccount { user_id: string; email: string; orgs: BalanceOrg[] }
