// 체험(목) 빌드용 스태프 API — 서버 없이 관리자 화면을 확인할 수 있게 메모리 예시 데이터로 동작한다.
// 상태 전이 규칙은 crates/core/src/staff.rs와 같게 흉내 낸다 (민감 항목 승인 → 2차 승인, 거절 → WITHDRAWN 등).
import { ApiError, messageForCode } from '../api/errors';
import { applicationPending, needsSecond } from './labels';
import type {
  ApprovalItem, DecisionInput, DecisionResult, DeliveryDecisionInput, DeliveryItem, DspItem, InquiryMessage, InquiryItem,
  Overview, PayoutItem, QueueRelease, ReleaseSheet, StaffDocument, StaffMe,
} from './api';

const ME = 'staff-me-0001';
const OTHER = 'staff-kim-0002';
const now = Date.now();
const iso = (hoursAgo: number) => new Date(now - hoursAgo * 3_600_000).toISOString();
const wait = <T>(v: T) => new Promise<T>(r => setTimeout(() => r(structuredClone(v)), 180));
const fail = (code: string, status = 422): never => { throw new ApiError(messageForCode(code, status), status, code); };
const uid = () => Math.random().toString(16).slice(2, 10) + '-mock';


interface MockRelease { q: QueueRelease; sheet: ReleaseSheet }

function sheetFor(q: QueueRelease, open: [string, string, string][], extra: Partial<ReleaseSheet> = {}): MockRelease {
  const tracks = q.release_type === 'SINGLE' ? 1 : 4;
  const checks = [
    ...open.map(([code, status, detail]) => ({ check_code: code, status, detail, at: q.submitted_at ?? iso(5) })),
    { check_code: 'S1_AUDIO_FORMAT', status: 'PASS', detail: 'FLAC 24bit / 48kHz', at: q.submitted_at ?? iso(5) },
    { check_code: 'S1_COVER_SIZE', status: 'PASS', detail: '3000x3000 JPG', at: q.submitted_at ?? iso(5) },
    { check_code: 'S2_META_CREDITS', status: open.some(o => o[0] === 'S2_META_CREDITS') ? 'REVIEW_REQUIRED' : 'PASS', detail: '', at: q.submitted_at ?? iso(5) },
  ].filter((c, i, a) => a.findIndex(x => x.check_code === c.check_code) === i);
  return {
    q,
    sheet: {
      release: { id: q.id, org_id: q.org_id, org_name: q.org_name, title: q.title, release_type: q.release_type, status: q.status, upc: null, revision_id: q.revision_id, submitted_at: q.submitted_at },
      application: {
        artist: q.artist ?? '', language: 'KOR', genre: 'K-Pop', release_date: q.release_date ?? '', label: q.org_name,
        p_line: `℗ 2026 ${q.org_name}`, c_line: `© 2026 ${q.org_name}`, platforms: q.platforms,
        declarations: { rights_confirmed: true, adult_confirmed: true, is_cover: false, is_remix: false, contains_samples: open.some(o => o[0] === 'S2_UNDECLARED_CONTENT'), ai_involved: false, explicit_content: false },
        tracks: Array.from({ length: tracks }, (_, i) => ({
          id: `${q.id}-t${i + 1}`, title: i === 0 ? q.title : `${q.title} (Track ${i + 1})`, version: '', disc_number: 1, track_number: i + 1,
          isrc: null, asset_kind: 'FLAC', parental_advisory: false,
          credits: [{ party_id: 'p1', role: 'COMPOSER' }, { party_id: 'p2', role: 'LYRICIST' }],
        })),
      },
      signed_application: { application_no: `AUD-20260926-${q.id.slice(0, 6).toUpperCase()}`, content_hash: 'a3f1c9e2'.repeat(8), signer_name: q.artist ?? '', signer_role: '본인', received_at: q.submitted_at ?? iso(5) },
      checks,
      open_checks: open.map(([code, status, detail]) => ({ check_code: code, status, detail })),
      advisories: [],
      overrides: [],
      notes: [],
      second_approvals: [],
      documents: [],
      delivery_staging: [],
      timeline: [
        { action: 'stage2.decision', reason: `REVIEW:${open.map(o => o[0]).join(',')}`, actor_user_id: null, actor_service: 'worker', at: iso(3) },
        { action: 'stage1.decision', reason: 'PASS', actor_user_id: null, actor_service: 'worker', at: iso(4) },
        { action: 'release.submitted', reason: 'SUBMIT', actor_user_id: 'artist', actor_service: null, at: q.submitted_at ?? iso(5) },
      ],
      ...extra,
    },
  };
}

const q = (p: Partial<QueueRelease> & Pick<QueueRelease, 'id' | 'title' | 'artist' | 'org_name'>): QueueRelease => ({
  org_id: `org-${p.id}`, release_type: 'SINGLE', status: 'STAGE2_REVIEW', revision_id: `rev-${p.id}`,
  release_date: '2026-10-17', submitted_at: iso(6), platforms: ['D-1', 'D-2', 'D-5', 'D-6'], agreement: null, ...p,
});

const releases: MockRelease[] = [
  sheetFor(q({ id: 'r1a2b3c4', title: '새벽의 온도', artist: '한결', org_name: '한결 뮤직', submitted_at: iso(26) }),
    [['S2_META_CREDITS', 'REVIEW_REQUIRED', '2번 트랙 작사가 표기가 아티스트명과 달라요.']]),
  sheetFor(q({ id: 'r2b3c4d5', title: 'Blue Hour', artist: 'NOVA', org_name: 'Nova Sound', release_type: 'EP', submitted_at: iso(9), platforms: ['D-1', 'D-3', 'D-5', 'D-6', 'D-7'] }),
    [['S2_INTEGRITY_DUP', 'REVIEW_REQUIRED', '기존 발매(ORG Moonlit)의 마스터와 지문 일치 92%'], ['S2_RIGHTS_SCOPE', 'REVIEW_REQUIRED', '권리 범위: 전 세계 배급 요청, 계약서는 대한민국 한정']]),
  sheetFor(q({ id: 'r3c4d5e6', title: '여름 끝에서', artist: '소월', org_name: '소월 프로젝트', submitted_at: iso(2) }),
    [['S2_UNDECLARED_CONTENT', 'REVIEW_REQUIRED', '샘플 사용 신호가 감지됐지만 신고 항목에 없어요.']]),
  sheetFor(q({ id: 'r4d5e6f7', title: 'Paper Moon', artist: 'Lumi', org_name: 'Lumi Records', status: 'READY_FOR_DELIVERY', agreement: 'REVIEW', submitted_at: iso(70) }), []),
  sheetFor(q({ id: 'r5e6f7a8', title: '고요한 밤의 노래', artist: '이안', org_name: '이안', status: 'STAGE2_CORRECTION', submitted_at: iso(50) }), []),
];

const approvals: (ApprovalItem & { status: string })[] = [{
  id: 'ap-7e21', org_id: 'org-r6', release_id: 'r6f7a8b9', revision_id: 'rev-r6', title: 'Midnight Drive',
  check_codes: ['AUDIO_SIMILAR_TO_EXISTING'], reason: '유사도 높은 기존 곡은 동일 아티스트의 라이브 버전으로 확인했어요.',
  requested_by: OTHER, expires_at: new Date(now + 40 * 3_600_000).toISOString(), at: iso(4), status: 'PENDING',
}];

const documents: StaffDocument[] = [
  { id: 'doc-1', org_id: 'org-r4d5e6f7', org_name: 'Lumi Records', release_id: 'r4d5e6f7', release_title: 'Paper Moon', kind: 'AGREEMENT', title: 'Paper Moon · AUDENIQ 디지털 음원 배급 신청·계약서', status: 'REVIEW', review_note: null, file_name: null, asset_id: null, signed_at: iso(25), row_version: 3, updated_at: iso(25) },
  { id: 'doc-2', org_id: 'org-r2b3c4d5', org_name: 'Nova Sound', release_id: 'r2b3c4d5', release_title: 'Blue Hour', kind: 'RIGHTS_PROOF', title: '샘플 사용 허락서', status: 'REVIEW', review_note: null, file_name: 'sample-license.pdf', asset_id: 'as-1', row_version: 2, updated_at: iso(8) },
  { id: 'doc-3', org_id: 'org-r3', org_name: '소월 프로젝트', release_id: 'r3c4d5e6', release_title: '여름 끝에서', kind: 'RIGHTS_PROOF', title: '공동 작곡 권리 확인서', status: 'NEEDS', review_note: '서명 페이지가 빠져 있어요.', file_name: 'co-writer.pdf', asset_id: 'as-2', row_version: 4, updated_at: iso(30) },
];

const inquiries: (InquiryItem & { messages: InquiryMessage[] })[] = [
  {
    id: 'iq-1', org_id: 'org-r1a2b3c4', org_name: '한결 뮤직', category: '발매·심사', release_id: 'r1a2b3c4', subject: '심사가 얼마나 걸리나요?', status: 'OPEN', created_at: iso(20), updated_at: iso(20),
    messages: [{ id: 'm1', author_kind: 'MEMBER', author_user: 'u1', body: '10월 17일 발매 예정인데 아직 심사 대기 상태라서요. 일정 안에 가능할까요?', created_at: iso(20) }],
  },
  {
    id: 'iq-2', org_id: 'org-r2b3c4d5', org_name: 'Nova Sound', category: '정산·지급', release_id: null, subject: '지급 계좌 변경 문의', status: 'OPEN', created_at: iso(7), updated_at: iso(7),
    messages: [{ id: 'm2', author_kind: 'MEMBER', author_user: 'u2', body: '법인 계좌로 바꾸고 싶은데 이번 달 정산부터 적용되나요?', created_at: iso(7) }],
  },
  {
    id: 'iq-3', org_id: 'org-x', org_name: '이안', category: '수정·테이크다운', release_id: null, subject: '커버 이미지 교체', status: 'ANSWERED', created_at: iso(60), updated_at: iso(40),
    messages: [
      { id: 'm3', author_kind: 'MEMBER', author_user: 'u3', body: '발매 후 커버를 바꿀 수 있나요?', created_at: iso(60) },
      { id: 'm4', author_kind: 'STAFF', author_user: OTHER, body: '발매·곡 관리에서 수정 신청을 올려 주시면 재심사 후 반영돼요.', created_at: iso(40) },
    ],
  },
];

const deliveries: DeliveryItem[] = [
  { package_id: 'pk-4d5e', dsp: 'D-1', org_id: 'org-r4', org_name: 'Lumi Records', release_id: 'r4d5e6f7', title: 'Paper Moon', readiness: 'READY', approval: 'PENDING', route_status: 'LIVE', route_reason: null, ern_is_preview: false, blockers: [], warnings: ['DSP_LOUDNESS_ADVISORY'], staged_at: iso(12) },
  { package_id: 'pk-4d5e', dsp: 'D-5', org_id: 'org-r4', org_name: 'Lumi Records', release_id: 'r4d5e6f7', title: 'Paper Moon', readiness: 'AWAITING_PARTNER', approval: 'PENDING', route_status: 'PENDING_ONBOARDING', route_reason: 'recipient DPID not registered', ern_is_preview: true, blockers: [], warnings: [], staged_at: iso(12) },
  { package_id: 'pk-9a0b', dsp: 'D-6', org_id: 'org-y', org_name: 'Nova Sound', release_id: 'r9', title: 'Afterglow', readiness: 'CONTENT_BLOCKED', approval: 'PENDING', route_status: null, route_reason: null, ern_is_preview: false, blockers: ['DDEX-PREFLIGHT-RELEASE-TYPE'], warnings: [], staged_at: iso(30) },
];

const DSP_NAMES = ['Melon', 'Genie', 'FLO', 'Bugs', 'Spotify', 'Apple Music / iTunes', 'YouTube Music', 'Amazon Music', 'TIDAL', 'Deezer', 'Qobuz'];
const dsps: DspItem[] = DSP_NAMES.map((name, i) => ({
  code: `D-${i + 1}`, dsp: `D-${i + 1}`, slug: name.toLowerCase(), name, region: i < 4 ? 'Kr' : 'Global', format: i < 4 ? 'PartnerSpec' : 'Ddex',
  lead_days: i < 4 ? 14 : 7, artwork_min_px: 3000, loudness_target_lufs: -14,
  route: i === 0 || i === 4 ? {
    transport: i === 0 ? 'SFTP' : 'DDEX_SFTP', activation_kind: i === 0 ? 'CONTRACTED' : 'SANDBOX', route_kind: 'DIRECT', delivery_enabled: i === 0,
    recipient_dpid_registered: i === 0, adapter_can_send: i === 0, onboarding_stage: i === 0 ? 'LIVE' : 'TESTING', onboarding_gaps: i === 0 ? [] : ['RECIPIENT_DPID'],
  } : null,
}));

const payouts: PayoutItem[] = [
  { id: 'po-1', org_id: 'org-r4', org_name: 'Lumi Records', amount: '184500', currency: 'KRW', status: 'REQUESTED', payout_order_id: null, created_at: iso(15) },
  { id: 'po-2', org_id: 'org-x', org_name: '이안', amount: '52300', currency: 'KRW', status: 'REQUESTED', payout_order_id: null, created_at: iso(40) },
];

releases.find(r => r.q.id === 'r4d5e6f7')!.sheet.documents.push(documents[0]);
/** 발매 심사 대기: 2차 검사에서 멈췄거나, 자동 검사를 통과했고 신청서가 검토 전 */
const awaiting = (r: MockRelease) => r.q.status === 'STAGE2_REVIEW' || (r.q.status === 'READY_FOR_DELIVERY' && applicationPending(r.q.agreement));

const find = (rid: string) => releases.find(r => r.q.id === rid) ?? fail('NOT_FOUND', 404);
const setStatus = (r: MockRelease, status: string) => { r.q.status = status; r.sheet.release.status = status; };
const audit = (r: MockRelease, action: string, reason: string) =>
  r.sheet.timeline.unshift({ action, reason, actor_user_id: ME, actor_service: null, at: new Date().toISOString() });

export const mockStaff = {
  me: () => wait<StaffMe>({ user_id: ME, role: 'ADMIN', duties: ['REVIEW', 'DOCUMENTS', 'INQUIRIES', 'DELIVERY'] }),
  overview: () => wait<Overview>({
    review: releases.filter(awaiting).length,
    correction: releases.filter(r => r.q.status.endsWith('_CORRECTION')).length,
    in_pipeline: 3,
    second_approvals: approvals.filter(a => a.status === 'PENDING').length,
    documents: documents.filter(d => d.kind === 'RIGHTS_PROOF' && d.status === 'REVIEW').length,
    inquiries: inquiries.filter(i => i.status === 'OPEN').length,
    deliveries_to_approve: deliveries.filter(d => d.approval === 'PENDING' && d.readiness !== 'CONTENT_BLOCKED').length,
    deliveries_blocked: deliveries.filter(d => d.readiness === 'CONTENT_BLOCKED').length,
    audio_advisories: deliveries.filter(d => d.approval === 'PENDING' && d.warnings.length).length,
    payout_requests: payouts.filter(p => p.status === 'REQUESTED').length,
  }),
  releases: (status: string) => wait({ items: releases.filter(r => (status === 'PENDING' ? awaiting(r) : r.q.status === status)).map(r => r.q) }),
  release: (rid: string) => wait(find(rid).sheet),
  decide: async (rid: string, i: DecisionInput): Promise<DecisionResult> => {
    const r = find(rid);
    if (!awaiting(r)) fail('RELEASE_NOT_IN_REVIEW');
    if (!i.reason.trim()) fail('DECISION_REASON_REQUIRED');
    if (r.q.status === 'READY_FOR_DELIVERY') {
      // 자동 검사를 통과한 새 발매 신청: 신청서(계약서)와 발매를 함께 결정
      const next = i.action === 'APPROVE' ? null : i.action === 'REJECT' ? 'WITHDRAWN' : 'STAGE3_CORRECTION';
      const agreement = i.action === 'APPROVE' ? 'APPROVED' : i.action === 'REJECT' ? 'REJECTED' : 'NEEDS';
      r.q.agreement = agreement;
      r.sheet.documents.filter(d => d.kind === 'AGREEMENT').forEach(d => Object.assign(d, { status: agreement, review_note: next ? i.reason : null }));
      r.sheet.notes.push({ id: uid(), revision_id: i.revision_id, check_code: null, decision: i.action, note: i.reason, author_user_id: ME, at: new Date().toISOString() });
      if (next) setStatus(r, next);
      audit(r, i.action === 'APPROVE' ? 'staff.approved' : i.action === 'REJECT' ? 'staff.rejected' : 'staff.correction_requested', next ? `READY_FOR_DELIVERY->${next}` : 'APPLICATION:APPROVED');
      return wait(next === 'WITHDRAWN' ? { result: 'REJECTED', status: 'WITHDRAWN' } : { result: 'APPLIED', reevaluation_queued: false });
    }
    const open = r.sheet.open_checks;
    const note = (decision: string, text: string, code: string | null = null) =>
      r.sheet.notes.push({ id: uid(), revision_id: i.revision_id, check_code: code, decision, note: text, author_user_id: ME, at: new Date().toISOString() });
    if (i.action === 'APPROVE') {
      if (open.some(needsSecond)) {
        if (approvals.some(a => a.release_id === rid && a.status === 'PENDING')) fail('SECOND_APPROVAL_ALREADY_PENDING');
        const a = { id: `ap-${uid()}`, org_id: r.q.org_id, release_id: rid, revision_id: i.revision_id, title: r.q.title, check_codes: open.map(c => c.check_code), reason: i.reason, requested_by: ME, expires_at: new Date(Date.now() + 72 * 3_600_000).toISOString(), at: new Date().toISOString(), status: 'PENDING' };
        approvals.push(a);
        r.sheet.second_approvals.unshift({ id: a.id, check_codes: a.check_codes, reason: a.reason, requested_by: ME, status: 'PENDING', decided_by: null, expires_at: a.expires_at, at: a.at });
        audit(r, 'staff.approval_requested', a.check_codes.join(','));
        return wait({ result: 'PENDING_SECOND_APPROVAL', approval_id: a.id, check_codes: a.check_codes });
      }
      note('APPROVE', i.reason);
      r.sheet.open_checks = [];
      setStatus(r, 'STAGE3_PREPARING');
      audit(r, 'staff.approved', `PASS:${open.length}`);
      return wait({ result: 'APPLIED', reevaluation_queued: true, passed: open.map(c => c.check_code) });
    }
    if (i.action === 'REQUEST_CORRECTION') {
      if (!open.length) fail('NOTHING_TO_CORRECT');
      note('REQUEST_CORRECTION', i.reason);
      i.notes?.forEach(n => note('REQUEST_CORRECTION', n.note, n.check_code));
      setStatus(r, 'STAGE2_CORRECTION');
      audit(r, 'staff.correction_requested', open.map(c => c.check_code).join(','));
      return wait({ result: 'APPLIED', reevaluation_queued: true });
    }
    note('REJECT', i.reason);
    i.notes?.forEach(n => note('REJECT', n.note, n.check_code));
    setStatus(r, 'WITHDRAWN');
    audit(r, 'staff.rejected', 'STAGE2_REVIEW->WITHDRAWN');
    return wait({ result: 'REJECTED', status: 'WITHDRAWN' });
  },
  withdraw: async (rid: string) => {
    const r = find(rid);
    setStatus(r, 'WITHDRAWN');
    audit(r, 'release.withdrawn', 'STAFF');
    return wait({ status: 'WITHDRAWN' });
  },
  reissue: async (rid: string) => {
    const r = find(rid);
    if (r.q.status !== 'READY_FOR_DELIVERY') fail('RELEASE_NOT_READY_FOR_DELIVERY');
    return fail('NO_VIRTUAL_IDENTIFIERS');
  },
  approvals: () => wait({ items: approvals.filter(a => a.status === 'PENDING') }),
  approve: async (aid: string) => {
    const a = approvals.find(x => x.id === aid && x.status === 'PENDING') ?? fail('APPROVAL_NOT_PENDING');
    if (a.requested_by === ME) fail('SECOND_APPROVER_MUST_DIFFER');
    a.status = 'APPROVED';
    return wait({ result: 'APPLIED' });
  },
  decline: async (aid: string) => {
    const a = approvals.find(x => x.id === aid && x.status === 'PENDING') ?? fail('APPROVAL_NOT_PENDING');
    a.status = 'DECLINED';
    return wait({ result: 'DECLINED' });
  },
  documents: (status: string) => wait({ items: documents.filter(d => d.kind === 'RIGHTS_PROOF' && d.status === status) }),
  reviewDocument: async (did: string, i: { status: 'APPROVED' | 'NEEDS'; note: string; row_version: number }) => {
    const d = documents.find(x => x.id === did);
    if (!d || d.row_version !== i.row_version) return fail('CONFLICT', 409);
    if (i.status === 'NEEDS' && !i.note.trim()) fail('REVIEW_NOTE_REQUIRED');
    Object.assign(d, { status: i.status, review_note: i.note.trim() || null, row_version: d.row_version + 1, updated_at: new Date().toISOString() });
    return wait({ status: d.status, row_version: d.row_version });
  },
  requestProof: async (org: string, i: { release_id: string; title: string; body: string }) => {
    const r = find(i.release_id);
    const d: StaffDocument = { id: `doc-${uid()}`, org_id: org, org_name: r.q.org_name, release_id: r.q.id, release_title: r.q.title, kind: 'RIGHTS_PROOF', title: i.title, status: 'AWAITING_DOCUMENTS', review_note: null, file_name: null, asset_id: null, row_version: 1, updated_at: new Date().toISOString() };
    documents.push(d);
    r.sheet.documents.push(d);
    return wait({ id: d.id });
  },
  inquiries: (status: string) => wait({ items: inquiries.filter(i => i.status === status).map(({ messages: _m, ...rest }) => rest) }),
  inquiry: async (iid: string) => {
    const i = inquiries.find(x => x.id === iid) ?? fail('NOT_FOUND', 404);
    const { messages, ...head } = i;
    return wait({ inquiry: head, messages });
  },
  reply: async (iid: string, body: string) => {
    const i = inquiries.find(x => x.id === iid) ?? fail('NOT_FOUND', 404);
    if (i.status === 'CLOSED') fail('INQUIRY_CLOSED');
    const m = { id: uid(), author_kind: 'STAFF', author_user: ME, body: body.trim(), created_at: new Date().toISOString() };
    i.messages.push(m);
    i.status = 'ANSWERED';
    i.updated_at = m.created_at;
    return wait({ id: m.id, status: 'ANSWERED' });
  },
  deliveries: (f: { approval: string; readiness?: string; dsp?: string }) =>
    wait({ items: deliveries.filter(d => d.approval === f.approval && (!f.readiness || d.readiness === f.readiness) && (!f.dsp || d.dsp === f.dsp)) }),
  ern: (pkg: string, dsp: string) => wait(`<?xml version="1.0" encoding="UTF-8"?>
<ern:NewReleaseMessage xmlns:ern="http://ddex.net/xml/ern/382" MessageSchemaVersionId="ern/382">
  <MessageHeader>
    <MessageId>${pkg}-${dsp}</MessageId>
    <MessageSender><PartyId>PADPIDA0000000000A</PartyId><PartyName><FullName>AUDENIQ</FullName></PartyName></MessageSender>
    <MessageRecipient><PartyId>PADPIDA${dsp.replace('-', '')}PREVIEW</PartyId></MessageRecipient>
  </MessageHeader>
  <!-- 체험 모드 예시 ERN -->
</ern:NewReleaseMessage>`),
  decideDelivery: async (pkg: string, dsp: string, i: DeliveryDecisionInput) => {
    const d = deliveries.find(x => x.package_id === pkg && x.dsp === dsp) ?? fail('NOT_FOUND', 404);
    if (i.action === 'HOLD' && !i.note?.trim()) fail('REVIEW_NOTE_REQUIRED');
    if (i.action === 'APPROVE') {
      if (d.readiness === 'CONTENT_BLOCKED') fail('DELIVERY_CONTENT_BLOCKED');
      if (d.warnings.length && !i.acknowledge_warnings) fail('WARNINGS_NOT_ACKNOWLEDGED');
    }
    d.approval = i.action === 'APPROVE' ? 'APPROVED' : 'HELD';
    return wait({ approval: d.approval });
  },
  restage: (_pkg: string) => wait({ job_id: uid() }),
  dsps: () => wait({ items: dsps }),
  payouts: (status: string) => wait({ items: payouts.filter(p => p.status === status) }),
};
