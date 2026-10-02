// 관리자(스태프) API — 백엔드 `/api/staff/*` (crates/core/src/staff.rs, docs/API.md "Staff portal").
// 아티스트 포털과 같은 세션 쿠키·CSRF를 쓰고, 권한은 identity.staff_members 역할로 서버가 판단한다.
// 체험(목) 빌드에서는 브라우저 메모리의 예시 데이터로 같은 화면을 확인할 수 있다.
import type { StudioDraft } from './application';
import { ApiError, messageForCode } from '../api/errors';
import { req } from '../api/http';
import { MOCK } from '../lib/mode';

export type StaffRole = 'ADMIN' | 'REVIEWER' | 'OPERATOR' | 'SUPPORT';
export type Duty = 'REVIEW' | 'DOCUMENTS' | 'INQUIRIES' | 'DELIVERY';

export interface StaffMe { user_id: string; role: StaffRole; duties: Duty[] }

export interface Overview {
  review: number; correction: number; in_pipeline: number; second_approvals: number;
  documents: number; inquiries: number; deliveries_to_approve: number; deliveries_blocked: number;
  audio_advisories: number; payout_requests: number;
}

export interface Page<T> { items: T[]; limit?: number; offset?: number }

export interface QueueRelease {
  id: string; org_id: string; org_name: string; title: string; release_type: string; status: string;
  revision_id: string | null; artist: string | null; release_date: string | null; submitted_at: string | null;
  platforms: string[];
  /** 발매 신청서(배급 계약서) 상태 — REVIEW/PREPARED면 발매 심사 대기 */
  agreement: string | null;
  /** 아티스트가 올린 커버 미리보기 (data URL) */
  cover?: string | null;
}

export interface Check { id?: string; stage?: number; original_status?: string; needs_second_approval?: boolean; check_code: string; status: string; detail: string | null; rule_version?: string; at?: string }
export interface Override {
  id: string; check_code: string; original_status: string; proposed_status: string; reason: string;
  actor_user_id: string; second_approver_user_id: string | null; at: string;
}
export interface ReviewNote {
  id: string; revision_id: string; check_code: string | null; decision: string; note: string; author_user_id: string; at: string;
}
export interface SecondApproval {
  id: string; check_codes: string[]; reason: string; requested_by: string; status: string;
  decided_by: string | null; expires_at: string; at: string; revision_id?: string; active: boolean;
}
export interface StaffDocument {
  id: string; org_id?: string; org_name?: string; release_id?: string | null; release_title?: string | null;
  kind: 'AGREEMENT' | 'RIGHTS_PROOF' | string; title: string; status: string; review_note: string | null;
  file_name: string | null; asset_id: string | null; signed_at?: string | null; row_version: number; updated_at: string;
  body?: string; signature?: string; signer_name?: string;
  electronic_record?: { document_no: string; rights_holder: string; signer_role: string; content_hash: string } | null;
}
export interface StagingRow {
  package_id: string; dsp: string; readiness: string; approval: string;
  checks: { code: string; severity?: string; class?: string; detail?: string; message?: string }[];
  route_status: string | null; route_reason: string | null; ern_message_id: string | null; ern_sha256: string | null;
  ern_is_preview: boolean; approval_by: string | null; approval_rule_version?: string | null; approval_note: string | null; approval_at: string | null; staged_at: string;
}
export interface TimelineEvent { action: string; reason: string | null; actor_user_id: string | null; actor_service: string | null; at: string }
export interface ReleaseTimelineItem {
  at: string; source: 'audit' | 'job' | 'check' | 'staff_decision' | 'dsp_request' | 'dsp_ack';
  kind: string; detail: Record<string, unknown>;
}
export interface ReleaseTimeline { release_id: string; items: ReleaseTimelineItem[]; truncated: boolean }
export interface ReviewContext {
  decision_kind: 'CHECKS' | 'APPLICATION' | null; allowed_actions: DecisionAction[];
  requires_second_approval: boolean; pending_second_approval_id: string | null;
  check_counts: Record<string, number>;
}
export interface Track {
  id: string; title: string; version: string; disc_number: number; track_number: number; isrc: string | null;
  asset_kind: string | null; parental_advisory: boolean; credits: { party_id: string; role: string }[];
}

export interface MeasuredAudio {
  duration_secs: number | null; sample_rate: number | null; channels: number | null; bits_per_sample: number | null;
}
export interface ReleaseSheet {
  track_audio?: Record<string, MeasuredAudio>;
  review_context?: ReviewContext;
  release: {
    id: string; org_id: string; org_name: string; title: string; release_type: string; status: string;
    upc: string | null; revision_id: string | null; submitted_at: string | null;
    cover?: string | null;
  };
  application: {
    artist?: string; language?: string; genre?: string; release_date?: string; original_date?: string;
    label?: string; p_line?: string; c_line?: string; territories?: unknown; platforms: string[];
    declarations?: Record<string, boolean> | null; tracks?: Track[] | null;
    options?: {
      express?: boolean; expressReason?: string; ai?: boolean; aiTool?: string;
      cover?: boolean; coverTracks?: { trackId: string; originalTitle: string; originalArtist: string; originalWriters: string }[];
      sample?: boolean; featured?: boolean; shared?: boolean; rerelease?: boolean;
      previousTitle?: string; previousId?: string;
      rereleaseKind?: string; previousDistributor?: string; previousUrl?: string; previousUpc?: string;
      previousAvailability?: string; rereleaseAudio?: string; rereleaseRights?: string; rereleaseNotes?: string;
      rereleaseTracks?: { trackId: string; previousIsrc: string }[];
    } | null;
  };
  /** 제출 리비전에 담긴 스튜디오 입력 전체 (크레딧·가사·부가서비스·권리 확인·서명 신청서) — 스튜디오 외 경로로 접수하면 없음 */
  draft?: StudioDraft | null;
  signed_application: {
    application_no: string; content_hash: string; signer_name: string; signer_role: string; received_at: string;
    form?: string; agreements?: string[];
    /** 서명 이미지 (PNG data URL) · 신청인 연락 이메일 (아티스트 정보, 없으면 계정 이메일) */
    signature?: string; contact_email?: string;
  } | null;
  checks: Check[];
  open_checks: Check[];
  advisories: Check[];
  overrides: Override[];
  notes: ReviewNote[];
  second_approvals: SecondApproval[];
  documents: StaffDocument[];
  delivery_staging: StagingRow[];
  timeline: TimelineEvent[];
}

export type DecisionAction = 'APPROVE' | 'REQUEST_CORRECTION' | 'REJECT';
export interface DecisionInput {
  action: DecisionAction; revision_id: string; reason: string; notes?: { check_code: string; note: string }[];
}
export type DecisionResult =
  | { result: 'APPLIED'; reevaluation_queued: boolean; passed?: string[] }
  | { result: 'PENDING_SECOND_APPROVAL'; approval_id: string; check_codes: string[] }
  | { result: 'REJECTED'; status: 'WITHDRAWN' };

export interface ApprovalItem {
  id: string; org_id: string; release_id: string; revision_id: string; title: string;
  check_codes: string[]; reason: string; requested_by: string; expires_at: string; at: string;
}

export interface InquiryItem {
  id: string; org_id: string; org_name: string; category: string; release_id: string | null;
  subject: string; status: 'OPEN' | 'ANSWERED' | 'CLOSED'; created_at: string; updated_at?: string;
}
export interface InquiryMessage { id: string; author_kind: 'MEMBER' | 'STAFF' | string; author_user: string | null; body: string; created_at: string }
export interface InquiryThread { inquiry: InquiryItem; messages: InquiryMessage[] }

export interface DeliveryItem {
  package_id: string; dsp: string; org_id: string; org_name: string; release_id: string; title: string;
  readiness: string; approval: string; route_status: string | null; route_reason: string | null;
  ern_is_preview: boolean; blockers: string[]; warnings: string[]; staged_at: string;
  /** 서버가 붙여 주는 플랫폼 이름 */
  dsp_name?: string;
}
export interface DeliveryDecisionInput { action: 'APPROVE' | 'HOLD'; note?: string; ern_sha256?: string; acknowledge_warnings?: boolean }

export interface DspItem {
  code: string; dsp: string; slug: string; name: string; region: string; format: string; lead_days: number;
  artwork_min_px: number; loudness_target_lufs: number;
  artwork_max_px?: number | null; audio_min_sample_rate?: number; audio_min_bits?: number; lossless_only?: boolean;
  requires_composer?: boolean; requires_lyricist?: boolean;
  /** 사용자·담당자에게 보이는 플랫폼 이름 (내부 코드는 화면에 쓰지 않는다) */
  name_ko?: string;
  ern_version?: string;
  deal?: { commercial_models: string[]; use_types: string[] };
  channel?: 'SFTP' | 'TRANSPORTER' | 'PARTNER_FEED';
  choreography?: 'batch' | 'release_by_release' | 'partner_feed';
  merlin_eligible?: boolean;
  content_id?: boolean; cover_license_required?: boolean; ai_policy?: boolean;
  category?: 'STREAMING' | 'SOCIAL' | 'STORE' | 'LICENSING'; planned_route?: 'DIRECT' | 'MERLIN';
  regional_review?: boolean; accepted_genres?: string[] | null;
  contract_route?: null | {
    route: 'DIRECT' | 'MERLIN'; merlin_eligible: boolean; merlin_agreement_signed: boolean; contract_live: boolean;
    updated_by: string | null; updated_at: string;
  };
  route: null | {
    transport: string; activation_kind: string; route_kind: string; delivery_enabled: boolean;
    recipient_dpid_registered: boolean; adapter_can_send: boolean; onboarding_stage: string | null; onboarding_gaps: string[];
  };
}

export interface PayoutItem {
  id: string; org_id: string; org_name: string; amount: string | number; currency: string; status: string;
  payout_order_id: string | null; created_at: string;
}

const qs = (o: Record<string, string | number | undefined | null>) => {
  const q = new URLSearchParams();
  for (const [k, v] of Object.entries(o)) if (v != null && v !== '') q.set(k, String(v));
  const s = q.toString();
  return s ? `?${s}` : '';
};
const id = (v: string) => encodeURIComponent(v);

/** ERN XML은 JSON이 아니라 원문 그대로 받는다 */
async function fetchText(path: string): Promise<string> {
  const API_BASE = (import.meta.env.VITE_API_BASE ?? '').replace(/\/$/, '');
  let res: Response;
  try {
    res = await fetch(`${API_BASE}${path}`, { credentials: 'include', cache: 'no-store', headers: { Accept: 'application/xml' } });
  } catch {
    throw new ApiError('네트워크 연결을 확인해 주세요.', 0, 'NETWORK');
  }
  const text = await res.text();
  if (!res.ok) {
    let code = '';
    try { code = JSON.parse(text)?.error?.code ?? ''; } catch { /* 본문이 JSON이 아님 */ }
    throw new ApiError(messageForCode(code || (res.status === 404 ? 'NOT_FOUND' : ''), res.status), res.status, code);
  }
  return text;
}

const remote = {
  me: () => req<StaffMe>('/api/staff/me', { quiet401: false }),
  overview: () => req<Overview>('/api/staff/overview'),
  releases: (status: string, offset = 0) => req<Page<QueueRelease>>(`/api/staff/releases${qs({ status, limit: 50, offset })}`),
  release: (rid: string) => req<ReleaseSheet>(`/api/staff/releases/${id(rid)}`),
  timeline: (rid: string, limit = 100) => req<ReleaseTimeline>(`/api/staff/releases/${id(rid)}/timeline${qs({ limit })}`),
  decide: (rid: string, body: DecisionInput) => req<DecisionResult>(`/api/staff/releases/${id(rid)}/decision`, { method: 'POST', body }),
  /** 아티스트 요청(문의)으로 발매 신청 취소 — 월 3회 직접 취소 한도와 무관 */
  withdraw: (rid: string, reason: string) => req<{ status: string }>(`/api/staff/releases/${id(rid)}/withdraw`, { method: 'POST', body: { reason } }),
  reissue: (rid: string, reason: string) => req<{ status: string }>(`/api/staff/releases/${id(rid)}/reissue-identifiers`, { method: 'POST', body: { reason } }),
  approvals: () => req<Page<ApprovalItem>>('/api/staff/approvals'),
  approve: (aid: string) => req<{ result: string }>(`/api/staff/approvals/${id(aid)}/approve`, { method: 'POST' }),
  decline: (aid: string) => req<{ result: string }>(`/api/staff/approvals/${id(aid)}/decline`, { method: 'POST' }),
  documents: (status: string) => req<Page<StaffDocument>>(`/api/staff/documents${qs({ status, limit: 100 })}`),
  reviewDocument: (did: string, body: { status: 'APPROVED' | 'NEEDS'; note: string; row_version: number }) =>
    req<{ status: string; row_version: number }>(`/api/staff/documents/${id(did)}/review`, { method: 'POST', body }),
  requestProof: (org: string, body: { release_id: string; title: string; body: string }) =>
    req<{ id: string }>(`/api/staff/orgs/${id(org)}/documents`, { method: 'POST', body }),
  inquiries: (status: string) => req<Page<InquiryItem>>(`/api/staff/inquiries${qs({ status, limit: 100 })}`),
  inquiry: (iid: string) => req<InquiryThread>(`/api/staff/inquiries/${id(iid)}`),
  reply: (iid: string, body: string) => req<{ id: string; status: string }>(`/api/staff/inquiries/${id(iid)}/reply`, { method: 'POST', body: { body } }),
  deliveries: (f: { approval: string; readiness?: string; dsp?: string }) =>
    req<Page<DeliveryItem>>(`/api/staff/deliveries${qs({ ...f, limit: 100 })}`),
  ern: (pkg: string, dsp: string) => fetchText(`/api/staff/deliveries/${id(pkg)}/${id(dsp)}/ern`),
  decideDelivery: (pkg: string, dsp: string, body: DeliveryDecisionInput) =>
    req<{ approval: string }>(`/api/staff/deliveries/${id(pkg)}/${id(dsp)}/decision`, { method: 'POST', body }),
  restage: (pkg: string) => req<{ job_id: string }>(`/api/staff/deliveries/${id(pkg)}/restage`, { method: 'POST' }),
  dsps: () => req<Page<DspItem>>('/api/staff/dsps'),
  payouts: (status: string) => req<Page<PayoutItem>>(`/api/staff/payouts${qs({ status, limit: 100 })}`),
};

export type StaffApi = typeof remote;

/** 체험 모드에서만 예시 데이터를 불러온다 — 실서버 빌드에는 mock.ts가 아예 들어가지 않는다 (MOCK은 빌드 때 정해지는 상수) */
function lazyMock(): StaffApi {
  let mod: Promise<StaffApi> | null = null;
  const load = () => (mod ??= import('./mock').then(m => m.mockStaff as StaffApi));
  return new Proxy({} as StaffApi, {
    get: (_, key) => (...args: unknown[]) => load().then(api => (api[key as keyof StaffApi] as (...a: unknown[]) => unknown)(...args)),
  });
}

export const staffApi: StaffApi = MOCK ? lazyMock() : remote;

/** SHA-256 (hex) — 승인 직전 ERN이 바뀌지 않았는지 서버에 함께 보낸다 */
export async function sha256Hex(text: string): Promise<string> {
  const buf = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(text));
  return [...new Uint8Array(buf)].map(b => b.toString(16).padStart(2, '0')).join('');
}
