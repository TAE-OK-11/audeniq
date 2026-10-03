// 권리 서류 서명 요청 — 발매 신청인이 문서를 만들어 권리자에게 보내고,
// 권리자가 본인확인 후 직접 서명한다 (서버: crates/core/src/signing.rs).
// 체험(목) 모드는 브라우저 저장소에 같은 흐름을 흉내 내며, 본인확인은 체험용 입력으로 대신한다.
import { orgPath, req } from './http';
import { ApiError, messageForCode } from './errors';
import { MOCK } from '../lib/mode';
import { createStore } from '../lib/store';
import { sha256Hex } from '../lib/application';
import { addDoc } from '../store/docs';
import { stampNow } from '../lib/date';
import { RIGHTS_FORM, rightsHashInput, type RightsDocumentKind, type SigningChannel } from '../lib/rightsDocument';

export type SigningStatus = 'PENDING' | 'VERIFIED' | 'SIGNED' | 'DECLINED' | 'CANCELLED' | 'EXPIRED';

export interface SigningRequestInput {
  release_id: string;
  document_no: string;
  document_kind: RightsDocumentKind;
  title: string;
  body: string;
  rights_holder: string;
  signer_name: string;
  signer_role: string;
  channel: SigningChannel;
}

/** 발매 신청인이 보는 요청 (링크 토큰·생년월일은 오지 않는다) */
export interface SigningRequest {
  id: string;
  release_id: string;
  document_no: string;
  form: string;
  document_kind: RightsDocumentKind;
  title: string;
  rights_holder: string;
  signer_name: string;
  signer_role: string;
  channel: SigningChannel;
  status: SigningStatus;
  expires_at: string;
  signed_at: string | null;
  document_id: string | null;
  certificate_hash: string | null;
  decline_reason: string;
  identity: { provider: string; method: string; verified_at: string } | null;
  created_at: string;
}

/** 링크는 만들 때 한 번만 받는다 */
export interface IssuedLink { id: string; token: string; expires_at: string; status: SigningStatus }

/** 서명하는 사람이 보는 문서 */
export interface SigningView {
  status: SigningStatus;
  form: string;
  document_no: string;
  document_kind: RightsDocumentKind;
  title: string;
  body: string;
  body_hash: string;
  rights_holder: string;
  signer_name: string;
  signer_role: string;
  channel: SigningChannel;
  expires_at: string;
  release_title: string | null;
  requested_by: string | null;
  artist: string | null;
  identity: { method: string; verified_at: string; name: string } | null;
  identity_provider: { ready: boolean; name: string | null };
  signature: string | null;
  signed_at: string | null;
  certificate_hash: string | null;
  events: { event: string; at: string; hash: string }[];
}

export interface SigningConsents { document: boolean; electronic_signature: boolean; privacy: boolean }

/** 서명 링크 주소 (같은 사이트의 /sign/{token}) */
export function signingUrl(token: string): string {
  return `${window.location.origin}/sign/${token}`;
}

// ---------------------------------------------------------------------------
// 실서버
// ---------------------------------------------------------------------------
const remote = {
  create: (i: SigningRequestInput) => req<IssuedLink>(orgPath('/signing-requests'), { method: 'POST', body: i }),
  list: async (releaseId?: string) => {
    const q = releaseId ? `?release_id=${encodeURIComponent(releaseId)}` : '';
    return (await req<{ items: SigningRequest[] }>(orgPath(`/signing-requests${q}`))).items;
  },
  reissue: (id: string, channel: SigningChannel) =>
    req<IssuedLink>(orgPath(`/signing-requests/${encodeURIComponent(id)}/reissue`), { method: 'POST', body: { channel } }),
  cancel: (id: string) => req<{ status: SigningStatus }>(orgPath(`/signing-requests/${encodeURIComponent(id)}/cancel`), { method: 'POST' }),
  view: (token: string) => req<SigningView>(`/api/sign/${encodeURIComponent(token)}`, { quiet401: true }),
  verify: (token: string, transactionId: string) =>
    req<{ status: SigningStatus; identity: SigningView['identity'] }>(`/api/sign/${encodeURIComponent(token)}/identity`, {
      method: 'POST', body: { transaction_id: transactionId }, quiet401: true,
    }),
  sign: (token: string, signature: string, consents: SigningConsents) =>
    req<{ status: SigningStatus; document_id: string; signed_at: string; certificate_hash: string }>(`/api/sign/${encodeURIComponent(token)}/sign`, {
      method: 'POST', body: { signature, consents }, quiet401: true,
    }),
  decline: (token: string, reason: string) =>
    req<{ status: SigningStatus }>(`/api/sign/${encodeURIComponent(token)}/decline`, { method: 'POST', body: { reason }, quiet401: true }),
};

// ---------------------------------------------------------------------------
// 체험(목) 모드 — 같은 규칙을 브라우저 안에서
// ---------------------------------------------------------------------------
interface MockSigning extends SigningRequest {
  token: string;
  body: string;
  body_hash: string;
  release_title: string;
  artist: string;
  signature: string;
  identity_name: string;
  events: { event: string; at: string; hash: string }[];
}
const mockStore = /* @__PURE__ */ createStore<MockSigning[]>([], { persist: 'mock.signing' });
const LINK_MS = 7 * 86_400_000;
const IN_PERSON_MS = 3_600_000;

function newToken(): string {
  const b = new Uint8Array(32);
  crypto.getRandomValues(b);
  return Array.from(b, x => x.toString(16).padStart(2, '0')).join('');
}
const fail = (code: string) => new ApiError(messageForCode(code), 400, code);
function effective(r: MockSigning): SigningStatus {
  return (r.status === 'PENDING' || r.status === 'VERIFIED') && Date.parse(r.expires_at) <= Date.now() ? 'EXPIRED' : r.status;
}
async function addEvent(r: MockSigning, event: string): Promise<void> {
  const at = new Date().toISOString();
  const prev = r.events.at(-1)?.hash ?? '';
  r.events.push({ event, at, hash: await sha256Hex(`${prev}|${r.id}|${event}|${at}`) });
}
function save(r: MockSigning) { mockStore.set(list => list.map(x => (x.id === r.id ? { ...r } : x))); }
function byToken(token: string): MockSigning {
  const r = mockStore.get().find(x => x.token === token);
  if (!r) throw new ApiError('서명 링크를 찾을 수 없어요.', 404, 'NOT_FOUND');
  return structuredClone(r);
}
function ensureOpen(r: MockSigning) {
  const s = effective(r);
  if (s === 'EXPIRED') throw fail('SIGNING_LINK_EXPIRED');
  if (s !== 'PENDING' && s !== 'VERIFIED') throw fail('SIGNING_REQUEST_CLOSED');
}
const strip = ({ token: _t, body: _b, body_hash: _h, release_title: _r, artist: _a, signature: _s, identity_name: _n, events: _e, ...rest }: MockSigning): SigningRequest =>
  ({ ...rest, status: effective({ ...rest, token: '', body: '', body_hash: '', release_title: '', artist: '', signature: '', identity_name: '', events: [] }) });

/** 체험 모드에서 서명 요청을 만들 때 발매명·아티스트를 같이 기억한다 */
let mockContext: { releaseTitle: string; artist: string } = { releaseTitle: '', artist: '' };
export function setMockSigningContext(c: { releaseTitle: string; artist: string }) { mockContext = c; }

const mock: typeof remote = {
  create: async i => {
    const existing = mockStore.get().find(x => x.document_no === i.document_no);
    const hash = await sha256Hex(JSON.stringify([i.title, i.body, i.rights_holder, i.signer_name, i.signer_role]));
    const token = newToken();
    const expires = new Date(Date.now() + (i.channel === 'IN_PERSON' ? IN_PERSON_MS : LINK_MS)).toISOString();
    if (existing) {
      if (existing.body_hash !== hash || existing.status !== 'PENDING') throw new ApiError(messageForCode('CONFLICT'), 409, 'CONFLICT');
      const r = structuredClone(existing);
      Object.assign(r, { token, expires_at: expires, channel: i.channel });
      await addEvent(r, 'LINK_REISSUED');
      save(r);
      return { id: r.id, token, expires_at: expires, status: 'PENDING' };
    }
    const r: MockSigning = {
      id: crypto.randomUUID(), release_id: i.release_id, document_no: i.document_no, form: RIGHTS_FORM,
      document_kind: i.document_kind, title: i.title, rights_holder: i.rights_holder, signer_name: i.signer_name,
      signer_role: i.signer_role, channel: i.channel, status: 'PENDING', expires_at: expires, signed_at: null,
      document_id: null, certificate_hash: null, decline_reason: '', identity: null, created_at: new Date().toISOString(),
      token, body: i.body, body_hash: hash, release_title: mockContext.releaseTitle, artist: mockContext.artist,
      signature: '', identity_name: '', events: [],
    };
    await addEvent(r, 'CREATED');
    mockStore.set(list => [r, ...list]);
    return { id: r.id, token, expires_at: expires, status: 'PENDING' };
  },
  list: async releaseId => mockStore.get().filter(r => !releaseId || r.release_id === releaseId).map(strip),
  reissue: async (id, channel) => {
    const found = mockStore.get().find(x => x.id === id);
    if (!found) throw new ApiError(messageForCode('NOT_FOUND'), 404, 'NOT_FOUND');
    if (found.status !== 'PENDING') throw fail('SIGNING_REQUEST_CLOSED');
    const r = structuredClone(found);
    const token = newToken();
    Object.assign(r, { token, channel, expires_at: new Date(Date.now() + (channel === 'IN_PERSON' ? IN_PERSON_MS : LINK_MS)).toISOString() });
    await addEvent(r, 'LINK_REISSUED');
    save(r);
    return { id, token, expires_at: r.expires_at, status: 'PENDING' };
  },
  cancel: async id => {
    const found = mockStore.get().find(x => x.id === id);
    if (!found) throw new ApiError(messageForCode('NOT_FOUND'), 404, 'NOT_FOUND');
    const r = structuredClone(found);
    ensureOpen(r);
    r.status = 'CANCELLED';
    await addEvent(r, 'CANCELLED');
    save(r);
    return { status: 'CANCELLED' };
  },
  view: async token => {
    const r = byToken(token);
    if (!r.events.some(e => e.event === 'VIEWED') && effective(r) !== 'EXPIRED' && (r.status === 'PENDING' || r.status === 'VERIFIED')) {
      await addEvent(r, 'VIEWED');
      save(r);
    }
    const status = effective(r);
    return {
      status, form: r.form, document_no: r.document_no, document_kind: r.document_kind, title: r.title, body: r.body,
      body_hash: r.body_hash, rights_holder: r.rights_holder, signer_name: r.signer_name, signer_role: r.signer_role,
      channel: r.channel, expires_at: r.expires_at, release_title: r.release_title, requested_by: r.artist, artist: r.artist,
      identity: r.identity ? { method: r.identity.method, verified_at: r.identity.verified_at, name: r.identity_name } : null,
      identity_provider: { ready: true, name: 'demo' },
      signature: status === 'SIGNED' ? r.signature : null, signed_at: r.signed_at, certificate_hash: r.certificate_hash,
      events: status === 'SIGNED' ? r.events : [],
    };
  },
  verify: async (token, transactionId) => {
    const r = byToken(token);
    ensureOpen(r);
    const [, name = '', birth = ''] = transactionId.split(':');
    if (!name.trim() || !/^\d{4}-\d{2}-\d{2}$/.test(birth)) throw fail('IDENTITY_NOT_VERIFIED');
    const norm = (s: string) => s.replace(/\s+/g, '').toLowerCase();
    if (norm(name) !== norm(r.signer_name)) {
      await addEvent(r, 'IDENTITY_FAILED');
      save(r);
      throw fail('IDENTITY_NAME_MISMATCH');
    }
    const at = new Date().toISOString();
    r.status = 'VERIFIED';
    r.identity = { provider: 'demo', method: 'DEMO', verified_at: at };
    r.identity_name = name.trim();
    await addEvent(r, 'IDENTITY_VERIFIED');
    save(r);
    return { status: 'VERIFIED', identity: { method: 'DEMO', verified_at: at, name: name.trim() } };
  },
  sign: async (token, signature, consents) => {
    if (!consents.document || !consents.electronic_signature || !consents.privacy) throw fail('SIGNING_CONSENT_REQUIRED');
    const r = byToken(token);
    ensureOpen(r);
    if (r.status !== 'VERIFIED' || !r.identity) throw fail('IDENTITY_NOT_VERIFIED');
    if (Date.parse(r.identity.verified_at) + 30 * 60_000 < Date.now()) throw fail('IDENTITY_EXPIRED');
    await addEvent(r, 'SIGNED');
    const at = r.events.at(-1)!.at;
    const contentHash = await sha256Hex(rightsHashInput(r.title, r.body, {
      document_no: r.document_no, form: r.form, document_kind: r.document_kind, rights_holder: r.rights_holder,
      signer_name: r.signer_name, signer_role: r.signer_role, signature,
    }));
    const certificate = await sha256Hex(JSON.stringify([r.id, r.body_hash, contentHash, r.identity, at, r.events.at(-1)!.hash]));
    const docId = crypto.randomUUID();
    const stampAt = stampNow(new Date(at));
    addDoc({
      id: docId, kind: 'rights', releaseId: r.release_id, releaseTitle: r.release_title, title: r.title, content: r.body,
      version: '2.0', created: stampAt, fileName: '', checked: true, checkedAt: stampAt,
      consentHistory: [{ time: stampAt, action: '권리자 본인확인 후 서명', version: '2.0' }],
      reviewHistory: [{ status: '권리자 서명 완료', time: stampAt, detail: '본인확인을 거친 권리자 서명 · AUDENIQ 검토 대기' }],
      reviewStatus: 'review', reviewNote: '', signerName: r.signer_name, localSignatureData: signature, localSignatureAt: stampAt,
      electronic: {
        document_no: r.document_no, form: r.form, document_kind: r.document_kind, rights_holder: r.rights_holder,
        signer_name: r.signer_name, signer_role: r.signer_role, content_hash: contentHash, signing_request_id: r.id,
        channel: r.channel, identity: { provider: 'demo', method: 'DEMO', name: r.identity_name, verified_at: r.identity.verified_at },
        certificate_hash: certificate,
      },
    });
    Object.assign(r, { status: 'SIGNED', signature, signed_at: at, document_id: docId, certificate_hash: certificate });
    save(r);
    return { status: 'SIGNED', document_id: docId, signed_at: at, certificate_hash: certificate };
  },
  decline: async (token, reason) => {
    const r = byToken(token);
    ensureOpen(r);
    r.status = 'DECLINED';
    r.decline_reason = reason.trim().slice(0, 500);
    await addEvent(r, 'DECLINED');
    save(r);
    return { status: 'DECLINED' };
  },
};

export const signingApi = MOCK ? mock : remote;
/** 체험 모드 목록이 바뀌면 다시 그리도록 */
export const useMockSigning = mockStore.use;

export const SIGNING_STATUS_LABEL: Record<SigningStatus, string> = {
  PENDING: '서명 대기',
  VERIFIED: '본인확인 완료 · 서명 대기',
  SIGNED: '서명 완료',
  DECLINED: '서명 거절',
  CANCELLED: '요청 취소',
  EXPIRED: '링크 만료',
};
