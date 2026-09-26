// 포털 API — 프로필·수령 계좌·문의·알림·서류·신청서·정산·리포트 (메인 서버)
// 공지·이벤트는 엣지 Worker가 D1에서 바로 서빙한다 (/api/notices, /api/events).
// 서버 응답을 화면에서 쓰는 모양(스토어 타입)으로 바꿔서 돌려준다.
import { orgPath, req } from './http';
import { remoteApi } from './remote';
import type { ProfileInfo } from '../store/profile';
import type { PaymentInfo } from '../store/payment';
import type { Notice } from '../store/support';
import type { Ticket } from '../store/tickets';
import type { DocRecord } from '../store/docs';
import type { Payout, Statement } from '../store/settlement';
import { stampNow } from '../lib/date';

/** 서버 시각(ISO) → 화면 표기용 'YYYY-MM-DD HH:MM' (로컬 시간) */
export function stamp(iso: unknown): string {
  if (typeof iso !== 'string' || !iso) return '';
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso.slice(0, 16).replace('T', ' ') : stampNow(d);
}

// ---------------------------------------------------------------------------
// 프로필
// ---------------------------------------------------------------------------
interface ServerProfile { display_name: string; contact_email: string; bio: string; country: string; row_version: number }
let profileVersion = 0;

export async function fetchProfile(): Promise<ProfileInfo> {
  const p = await req<ServerProfile>('/api/me/profile');
  profileVersion = p.row_version;
  return { name: p.display_name, email: p.contact_email, bio: p.bio, country: p.country === 'ZZ' ? 'OTHER' : p.country };
}

export async function saveProfile(input: ProfileInfo): Promise<void> {
  // 화면의 '기타' 국가는 ISO 사용자 지정 코드 ZZ로 저장
  const p = { ...input, country: input.country === 'OTHER' ? 'ZZ' : input.country };
  try {
    const r = await req<{ row_version: number }>('/api/me/profile', {
      method: 'PUT',
      body: { display_name: p.name, contact_email: p.email, bio: p.bio, country: p.country, row_version: profileVersion },
    });
    profileVersion = r.row_version;
  } catch (e) {
    // 다른 탭·기기에서 먼저 바뀌었으면 최신 버전을 받아 한 번 더 저장
    if ((e as { code?: string }).code === 'CONFLICT') {
      await fetchProfile();
      const r = await req<{ row_version: number }>('/api/me/profile', {
        method: 'PUT',
        body: { display_name: p.name, contact_email: p.email, bio: p.bio, country: p.country, row_version: profileVersion },
      });
      profileVersion = r.row_version;
      return;
    }
    throw e;
  }
}

// ---------------------------------------------------------------------------
// 수령 계좌 — 계좌번호 전체는 보내기만 하고 받지 않는다
// ---------------------------------------------------------------------------
const PAYEE_TO: Record<PaymentInfo['type'], string> = { personal: 'INDIVIDUAL', business: 'SOLE_PROPRIETOR', corporate: 'CORPORATION' };
const PAYEE_FROM: Record<string, PaymentInfo['type']> = { INDIVIDUAL: 'personal', SOLE_PROPRIETOR: 'business', CORPORATION: 'corporate' };

interface ServerAccount { registered: boolean; payee_type?: string; holder_name?: string; bank_name?: string; account_last4?: string; registered_at?: string }

export async function fetchPayoutAccount(): Promise<PaymentInfo | null> {
  const a = await req<ServerAccount>(orgPath('/payout-account'));
  if (!a.registered) return null;
  return {
    recipient: a.holder_name ?? '', type: PAYEE_FROM[a.payee_type ?? ''] ?? 'personal', bank: a.bank_name ?? '',
    accountNumber: `••••${a.account_last4 ?? ''}`, last4: a.account_last4 ?? '', registeredAt: stamp(a.registered_at),
  };
}

export async function savePayoutAccount(p: PaymentInfo, accountNumber: string): Promise<void> {
  await req(orgPath('/payout-account'), {
    method: 'PUT',
    body: { payee_type: PAYEE_TO[p.type], holder_name: p.recipient, bank_name: p.bank, account_number: accountNumber },
  });
}

// ---------------------------------------------------------------------------
// 문의
// ---------------------------------------------------------------------------
const CATEGORY_TO: Record<string, string> = {
  '발매·심사': 'RELEASE', '수정·테이크다운': 'RELEASE', '정산·지급': 'SETTLEMENT', '계약·권리': 'CONTRACT', '계정·기타': 'ACCOUNT',
};
const CATEGORY_FROM: Record<string, string> = {
  RELEASE: '발매·심사', SETTLEMENT: '정산·지급', CONTRACT: '계약·권리', ACCOUNT: '계정·기타', OTHER: '계정·기타',
};
const TICKET_STATUS: Record<string, string> = { OPEN: '답변 대기', ANSWERED: '답변 완료', CLOSED: '종료' };

interface ServerInquiry {
  id: string; category: string; release_id: string | null; release_title: string | null; subject: string;
  status: string; created_at: string; updated_at: string;
  messages: number | { id: string; author_kind: 'ARTIST' | 'STAFF'; body: string; created_at: string }[];
}

export interface TicketMessage { id: string; from: 'artist' | 'staff'; body: string; time: string }

function toTicket(i: ServerInquiry, first = ''): Ticket {
  return {
    id: i.id, category: CATEGORY_FROM[i.category] ?? '계정·기타', releaseId: i.release_id ?? '', releaseTitle: i.release_title ?? '',
    subject: i.subject, body: first, created: stamp(i.created_at).slice(0, 10), status: TICKET_STATUS[i.status] ?? i.status,
  };
}

export async function fetchInquiries(): Promise<Ticket[]> {
  const r = await req<{ items: ServerInquiry[] }>(orgPath('/inquiries'));
  return r.items.map(i => toTicket(i));
}

export async function fetchInquiry(id: string): Promise<{ ticket: Ticket; messages: TicketMessage[] }> {
  const i = await req<ServerInquiry>(orgPath(`/inquiries/${encodeURIComponent(id)}`));
  const list = Array.isArray(i.messages) ? i.messages : [];
  return {
    ticket: toTicket(i, list[0]?.body ?? ''),
    messages: list.map(m => ({ id: m.id, from: m.author_kind === 'STAFF' ? 'staff' : 'artist', body: m.body, time: stamp(m.created_at) })),
  };
}

export async function createInquiry(t: Pick<Ticket, 'category' | 'releaseId' | 'subject' | 'body'>): Promise<string> {
  const r = await req<{ id: string }>(orgPath('/inquiries'), {
    method: 'POST',
    body: { category: CATEGORY_TO[t.category] ?? 'OTHER', release_id: t.releaseId || null, subject: t.subject, body: t.body },
  });
  return r.id;
}

export async function replyInquiry(id: string, body: string): Promise<void> {
  await req(orgPath(`/inquiries/${encodeURIComponent(id)}/messages`), { method: 'POST', body: { body } });
}

export async function closeInquiry(id: string): Promise<void> {
  await req(orgPath(`/inquiries/${encodeURIComponent(id)}/close`), { method: 'POST' });
}

// ---------------------------------------------------------------------------
// 알림
// ---------------------------------------------------------------------------
const KIND_LABEL: Record<string, string> = {
  RELEASE: '발매', DOCUMENT: '서류', SETTLEMENT: '지급', INQUIRY: '문의', ACCOUNT: '계정', SYSTEM: '안내',
};
interface ServerNotification { id: string; kind: string; title: string; detail: string; link: string; created_at: string; read: boolean }

export async function fetchNotifications(): Promise<Notice[]> {
  const r = await req<{ items: ServerNotification[] }>(orgPath('/notifications'));
  return r.items.map(n => ({
    id: n.id, kind: KIND_LABEL[n.kind] ?? n.kind, title: n.title, detail: n.detail, time: stamp(n.created_at), read: n.read, link: n.link || undefined,
  }));
}

export async function markNotifications(ids: string[] | 'all'): Promise<void> {
  await req(orgPath('/notifications/read'), { method: 'POST', body: ids === 'all' ? { all: true } : { ids } });
}

// ---------------------------------------------------------------------------
// 서류 (계약서 서명, 권리 증빙 제출)
// ---------------------------------------------------------------------------
interface ServerDocument {
  id: string; kind: 'AGREEMENT' | 'RIGHTS_PROOF'; release_id: string | null; release_title: string | null; title: string; version: string;
  body: string; status: string; review_note: string; asset_id: string | null; file_name: string; checked_at: string | null;
  signer_name: string; signature: string; signed_at: string | null; row_version: number; created_at: string; updated_at: string;
}

const DOC_STATUS: Record<string, DocRecord['reviewStatus']> = {
  AWAITING_DOCUMENTS: 'awaiting_documents', REVIEW: 'review', PREPARED: 'prepared', APPROVED: 'approved', NEEDS: 'needs', SIGNED: 'approved',
};

function toDoc(d: ServerDocument): DocRecord {
  const history: DocRecord['reviewHistory'] = [{ status: '문서 생성', time: stamp(d.created_at), detail: d.kind === 'AGREEMENT' ? '신청 내용이 계약서로 정리됐어요.' : '제출이 필요한 서류예요.' }];
  if (d.status === 'APPROVED' || d.status === 'SIGNED') history.push({ status: '검토 완료', time: stamp(d.updated_at), detail: 'AUDENIQ 검토가 끝났어요.' });
  return {
    id: d.id, kind: d.kind === 'AGREEMENT' ? 'agreements' : 'rights', title: d.title, releaseId: d.release_id ?? undefined,
    releaseTitle: d.release_title ?? '', version: d.version, created: stamp(d.created_at).slice(0, 10), content: d.body,
    fileName: d.file_name, fileBlob: null, uploadedAt: d.asset_id ? stamp(d.updated_at) : undefined,
    approvedAt: d.status === 'APPROVED' || d.status === 'SIGNED' ? stamp(d.updated_at) : undefined,
    checked: !!d.checked_at, checkedAt: stamp(d.checked_at),
    consentHistory: d.checked_at ? [{ time: stamp(d.checked_at), action: '내용 확인', version: d.version }] : [],
    reviewHistory: history, reviewStatus: DOC_STATUS[d.status] ?? 'review', reviewNote: d.review_note,
    signerName: d.signer_name, localSignatureData: d.signature, localSignatureAt: stamp(d.signed_at),
    rowVersion: d.row_version,
  };
}

export async function fetchDocuments(): Promise<DocRecord[]> {
  const r = await req<{ items: ServerDocument[] }>(orgPath('/documents'));
  return r.items.map(toDoc);
}

export async function checkDocument(id: string): Promise<number> {
  const r = await req<{ row_version: number }>(orgPath(`/documents/${encodeURIComponent(id)}/check`), { method: 'POST' });
  return r.row_version;
}

export async function signDocument(id: string, signerName: string, signature: string, rowVersion: number): Promise<void> {
  await req(orgPath(`/documents/${encodeURIComponent(id)}/sign`), {
    method: 'POST', body: { signer_name: signerName, signature, row_version: rowVersion },
  });
}

export async function createDocument(releaseId: string, title: string, body: string, file: File | null): Promise<string> {
  const up = file ? await remoteApi.uploadFile(file, 'DOCUMENT') : null;
  const r = await req<{ id: string }>(orgPath('/documents'), {
    method: 'POST',
    body: { release_id: releaseId, title, body, asset_id: up?.assetId ?? null, file_name: file ? file.name.slice(0, 200) : '' },
  });
  return r.id;
}

export async function submitProof(id: string, file: File, rowVersion: number, onProgress?: (r: number) => void): Promise<void> {
  const up = await remoteApi.uploadFile(file, 'DOCUMENT', onProgress);
  await req(orgPath(`/documents/${encodeURIComponent(id)}/proof`), {
    method: 'POST', body: { asset_id: up.assetId, file_name: file.name.slice(0, 200), row_version: rowVersion },
  });
}

// ---------------------------------------------------------------------------
// 정산 — 서버 원장 기준 (읽기 전용) + 지급 요청
// ---------------------------------------------------------------------------
export interface FinanceSummary { payable: number; pending: number; available: number; minimum: number; accountRegistered: boolean }

export async function fetchFinance(): Promise<{ summary: FinanceSummary; statements: Statement[]; payouts: Payout[] }> {
  const [s, st, po] = await Promise.all([
    req<{ payable: string; pending: string; available: string; minimum_payout: string; account_registered: boolean }>(orgPath('/finance/summary')),
    req<{ items: { id: string; description: string; source: Record<string, unknown>; created_at: string; amount: string }[] }>(orgPath('/finance/statements')),
    req<{ items: { id: string; amount: string; note: string; status: string; order_status: string | null; created_at: string }[] }>(orgPath('/finance/payouts')),
  ]);
  return {
    summary: {
      payable: Number(s.payable), pending: Number(s.pending), available: Number(s.available),
      minimum: Number(s.minimum_payout), accountRegistered: s.account_registered,
    },
    statements: st.items.map(t => ({
      id: t.id,
      period: typeof t.source?.period === 'string' ? t.source.period : stamp(t.created_at).slice(0, 7),
      platform: typeof t.source?.dsp === 'string' ? t.source.dsp : '정산',
      amount: Number(t.amount), note: t.description, created: stamp(t.created_at).slice(0, 10),
    })),
    payouts: po.items.map(p => ({
      id: p.id, amount: Number(p.amount), note: p.note, created: stamp(p.created_at).slice(0, 10),
      status: p.order_status === 'SETTLED' ? 'sent'
        : p.status === 'REJECTED' || p.status === 'CANCELLED' || p.order_status === 'FAILED' || p.order_status === 'RETURNED' ? 'failed'
        : p.status === 'ORDERED' ? 'processing' : 'requested',
    })),
  };
}

export async function requestPayout(amount: number): Promise<void> {
  const key = `studio-payout-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
  await req(orgPath('/finance/payouts'), { method: 'POST', body: { amount: String(amount), idempotency_key: key } });
}

// ---------------------------------------------------------------------------
// 리포트 (매칭된 플랫폼 리포트)
// ---------------------------------------------------------------------------
export interface ServerReports {
  by_month: { month: string; streams: string; revenue: string }[];
  by_dsp: { dsp: string; streams: string; revenue: string }[];
  by_release: { release_id: string; title: string; streams: string; revenue: string }[];
  rows: { month: string; dsp: string; release_id: string | null; release: string; streams: string; revenue: string }[];
}
export const fetchReports = () => req<ServerReports>(orgPath('/reports'));

// ---------------------------------------------------------------------------
// 공지·이벤트 (엣지 Worker + D1)
// ---------------------------------------------------------------------------
export interface ContentNotice { id: string; title: string; body: string; pinned: boolean; published_at: string }
export interface ContentEvent {
  id: string; title: string; summary: string; body: string; place: string;
  starts_on: string; ends_on: string | null; link_url: string | null; status: 'upcoming' | 'ongoing' | 'ended';
}
export const fetchNotices = () => req<{ items: ContentNotice[] }>('/api/notices', { quiet401: true }).then(r => r.items);
export const fetchEvents = () => req<{ items: ContentEvent[] }>('/api/events', { quiet401: true }).then(r => r.items);
