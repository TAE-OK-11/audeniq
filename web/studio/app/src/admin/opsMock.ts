// 운영 도구 체험 데이터 (VITE_MOCK=true 빌드에서만 불러온다)
import type { OpsArea, OpsIndex } from './ops';

const OPS: [OpsArea, string, number, boolean, boolean][] = [
  ['staff', 'list', 3, false, false], ['staff', 'grant', 4, true, true], ['staff', 'revoke', 4, true, true], ['staff', 'reset-mfa', 4, true, true],
  ['protected', 'list', 2, false, false], ['protected', 'add', 3, true, false], ['protected', 'activate', 3, true, false], ['protected', 'deactivate', 3, true, false],
  ['protected', 'alias', 3, true, false], ['protected', 'remove-alias', 3, true, false], ['protected', 'grant-exception', 3, true, false], ['protected', 'revoke-exception', 3, true, false],
  ['developer-apps', 'list', 3, false, false], ['developer-apps', 'approve', 3, true, false], ['developer-apps', 'return', 3, true, false],
  ['ai-clients', 'list', 3, false, false], ['ai-clients', 'add', 3, true, true], ['ai-clients', 'disable', 3, true, false],
  ['ai-limits', 'list', 2, false, false], ['ai-limits', 'set', 2, true, false], ['ai-limits', 'clear', 2, true, false],
  ['partners', 'list', 2, false, false], ['partners', 'status', 2, false, false], ['partners', 'probe', 3, true, false], ['partners', 'route', 3, true, false], ['partners', 'go-live', 4, true, true],
  ['identifiers', 'list', 3, false, false], ['identifiers', 'register', 4, true, true],
  ['payouts', 'list', 2, false, false], ['payouts', 'order', 2, true, false], ['payouts', 'approve', 2, true, true],
  ['royalty', 'show', 2, false, false], ['royalty', 'post', 2, true, true],
];

const now = Date.now();
const iso = (minutesAgo: number) => new Date(now - minutesAgo * 60_000).toISOString();
const staff = [
  { email: 'staff@audeniq.com', user_id: 'staff-me-0001', role: 'OWNER', status: 'ACTIVE', granted_by: 'cli', granted_at: iso(60 * 24 * 60), revoked_at: null },
  { email: 'lead@audeniq.com', user_id: 'u-lead', role: 'ADMIN', status: 'ACTIVE', granted_by: 'cli', granted_at: iso(60 * 24 * 45), revoked_at: null },
  { email: 'review@audeniq.com', user_id: 'u-review', role: 'REVIEWER', status: 'ACTIVE', granted_by: 'staff:u-owner', granted_at: iso(60 * 24 * 20), revoked_at: null },
  { email: 'ops@audeniq.com', user_id: 'u-ops', role: 'OPERATOR', status: 'ACTIVE', granted_by: 'staff:u-owner', granted_at: iso(60 * 24 * 12), revoked_at: null },
  { email: 'cs@audeniq.com', user_id: 'u-cs', role: 'SUPPORT', status: 'ACTIVE', granted_by: 'staff:u-owner', granted_at: iso(60 * 30), revoked_at: null },
  { email: 'former@audeniq.com', user_id: 'u-old', role: 'SUPPORT', status: 'REVOKED', granted_by: 'cli', granted_at: iso(60 * 24 * 200), revoked_at: iso(60 * 24 * 40) },
];
const DATA: Record<string, unknown> = {
  'staff/list': staff,
  'protected/list': [
    { id: 'p1', name: 'IU', active: true, match_mode: 'TOKEN', action: 'BLOCK', note: '소속사 요청 (2026-09)', aliases: [{ alias: '아이유', kind: 'NAME', match_mode: 'CONTAINS', action: 'BLOCK' }, { alias: 'Lee Ji-eun', kind: 'NAME', match_mode: 'TOKEN', action: 'BLOCK' }], exceptions: [{ org_id: '7c1e9a40-2b51-4c6e-9d0a-1f00aa000001', reason: '본인 소속사', granted_by: 'cli', revoked: false }] },
    { id: 'p2', name: 'NewJeans', active: true, match_mode: 'CONTAINS', action: 'REVIEW', note: null, aliases: [{ alias: '뉴진스', kind: 'NAME', match_mode: 'CONTAINS', action: 'REVIEW' }], exceptions: [] },
    { id: 'p3', name: 'Old Name', active: false, match_mode: 'CONTAINS', action: 'BLOCK', note: '해제 요청으로 끔', aliases: [], exceptions: [] },
  ],
  'developer-apps/list': { items: [
    { client_id: 'audeniq-app-1', name: 'Release Calendar', owner_user_id: 'u-a', owner_email: 'dev@label.kr', visibility: 'REVIEW', website: 'https://calendar.label.kr', description: '레이블 소속 아티스트의 발매 일정을 한 화면에서 봐요.', redirect_uris: ['https://calendar.label.kr/oauth/callback'], review_requested_at: iso(180), reviewed_at: null, review_note: null, created_at: iso(60 * 24 * 3) },
  ] },
  'ai-clients/list': { items: [{ client_id: 'audeniq-agent-7f3a', name: 'Partner agent', password_grant: false, auth: 'client_secret_basic', redirect_uris: [], created_at: iso(60 * 24 * 30), disabled_at: null }] },
  'ai-limits/list': { items: [{ user_id: 'u-artist', email: 'artist@example.com', daily_limit: 400, note: '문의 #1024', set_by: 'staff:u-ops', updated_at: iso(90) }] },
  'partners/list': [
    { partner_id: 'merlin', display_name: 'Merlin', dsp_id: null, transport: 'SFTP', activation_kind: 'CONTRACTED', delivery_enabled: false },
    { partner_id: 'melon', display_name: 'Melon', dsp_id: 'd-5', transport: 'API', activation_kind: 'CONTRACTED', delivery_enabled: true },
  ],
  'partners/status': { partner_id: 'merlin', stage: 'TESTING', dpid_registered: true, endpoint_url: 'sftp://ingest.merlin.example', endpoint_health: 'OK', credential_kind: 'SSH_KEY', credential_status: 'STORED', test_ern_validated_at: iso(60 * 24), test_ack_parsed_at: null, contract_signed_at: null, contract_ref: null, gaps: ['TEST_ACK', 'CONTRACT'] },
  'identifiers/list': [
    { kind: 'ISRC', mode: 'VIRTUAL', prefix: 'QZ-TST', active: true, created_at: iso(60 * 24 * 120), issued: { '26': 412 } },
    { kind: 'UPC', mode: 'VIRTUAL', prefix: '0000000', active: true, created_at: iso(60 * 24 * 120), issued: { '0': 188 } },
  ],
  'payouts/list': [
    { request_id: 'r1', org_id: '7c1e9a40-2b51-4c6e-9d0a-1f00aa000001', amount: '320000', fee: '1000', net: '319000', status: 'REQUESTED', account: { bank_name: '기업은행', account_masked: '***-***-1234', holder_name: '김하늘' }, created_at: iso(240), order_id: null, order_status: null },
    { request_id: 'r2', org_id: '1a2b3c4d-0000-4c6e-9d0a-1f00aa000002', amount: '1250000', fee: '1000', net: '1249000', status: 'REQUESTED', account: { bank_name: '국민은행', account_masked: '***-**-5678', holder_name: '이도윤' }, created_at: iso(60 * 26), order_id: 'o2', order_status: 'PENDING_APPROVAL' },
  ],
  'royalty/show': { report_id: 'rep-1', dsp_id: 'spotify', status: 'NORMALIZED', currency: 'USD', source_hash: 'ab12', source_amount: '1824.55', received_amount: '0', unresolved: 3 },
};

const wait = <T>(v: T) => new Promise<T>(r => setTimeout(() => r(structuredClone(v)), 200));

export const mockOps = {
  index: () => wait<OpsIndex>({ available: true, role: 'OWNER', level: 4, ops: OPS.map(([area, op, level, write, step_up]) => ({ area, op, level, write, step_up, finance: area === 'payouts' || area === 'royalty', allowed: true })) }),
  read: <T>(area: OpsArea, op: string, _q: Record<string, string | undefined>) => wait(DATA[`${area}/${op}`] as T),
  write: <T>(area: OpsArea, op: string, _body: Record<string, unknown>) =>
    wait((area === 'ai-clients' && op === 'add' ? { client_id: 'audeniq-agent-demo', client_secret: 'demo-secret-shown-once' } : { ok: true }) as T),
};
