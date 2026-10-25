// 부가서비스 — 서버 crates/core/src/addons.rs. 뮤직비디오 배급·심의, 신속 발매, AI 가사 싱크를 신청하고
// 정산금(정산 잔액에서 바로 차감) 또는 카드(토스페이먼츠)로 결제한다. 체험 모드는 같은 모양을 브라우저 안에서 흉내 낸다.
import { orgPath, req } from '../api/http';
import { MOCK } from './mode';

export type Service = 'MV_DISTRIBUTION' | 'EXPRESS_RELEASE' | 'LYRIC_SYNC';
export type MvTier = 'BASIC' | 'REVIEW' | 'EXPRESS';
export type OrderStatus = 'AWAITING_PAYMENT' | 'SUBMITTED' | 'IN_PROGRESS' | 'DELIVERED' | 'COMPLETED' | 'CANCELLED' | 'REJECTED';

export interface AddonOrder {
  id: string; service: Service; tier: '' | MvTier;
  release_id: string | null; release_title: string | null;
  track_ids: string[]; tracks: { id: string; title: string }[];
  title: string; detail: Record<string, unknown>;
  quantity: number; unit_price: number; amount: number;
  status: OrderStatus; pay_method: 'SETTLEMENT' | 'CARD' | null; paid_at: string | null; refunded_at: string | null;
  result: { rating?: string; rating_no?: string; note?: string; links?: Record<string, string> };
  resolution: string; created_at: string; updated_at: string;
  events: { status: string; note: string; by_staff: boolean; at: string }[];
}

export interface AddonState {
  items: AddonOrder[];
  prices: { mv: { tier: MvTier; price: number }[]; express_release: number; lyric_sync: number };
  /** 카드 결제를 쓸 수 있는지 (서버에 토스 키가 있을 때) */
  card: boolean;
  /** 지금 출금할 수 있는 정산 잔액 (원) */
  balance: number;
  mv_platforms: string[];
}

export interface TrackRow { id: string; title: string; track_number: number; has_lyrics: boolean; lrc: string | null; source: 'ARTIST' | 'AI' | null; registered: string[] }

export const SERVICE_LABEL: Record<Service, string> = {
  MV_DISTRIBUTION: '뮤직비디오 배급', EXPRESS_RELEASE: '신속 발매', LYRIC_SYNC: 'AI 가사 싱크',
};
export const TIER: Record<MvTier, { label: string; desc: string }> = {
  BASIC: { label: '배급만', desc: '이미 영상물등급위원회 등급을 받은 영상을 주요 영상 플랫폼에 보내요.' },
  REVIEW: { label: '배급 + 심의', desc: '영상물등급위원회 등급 분류 신청을 대신하고, 등급이 나오면 바로 배급해요.' },
  EXPRESS: { label: '배급 + 심의 + 급행', desc: '심의 신청과 배급을 가장 먼저 처리해요. 발매일이 가까울 때.' },
};
export const RATING_LABEL: Record<string, string> = { ALL: '전체관람가', '12': '12세 이상', '15': '15세 이상', '18': '청소년 관람불가' };
export const LYRIC_SERVICE_LABEL: Record<string, string> = {
  musixmatch: 'Musixmatch', lyricfind: 'LyricFind', melon: '멜론', genie: '지니', bugs: '벅스', flo: 'FLO', vibe: '바이브',
};

/** 진행 단계 카드 — 결제 → 접수 → 진행 중 → 전달 → 완료 (취소·진행 불가는 멈춘 자리) */
export const STEPS = ['결제', '접수', '진행 중', '결과 전달', '완료'];
const STEP_OF: Record<string, number> = { AWAITING_PAYMENT: 0, SUBMITTED: 1, IN_PROGRESS: 2, DELIVERED: 3, COMPLETED: 4 };
export function orderStage(o: Pick<AddonOrder, 'status' | 'events'>): { steps: string[]; current: number; complete: boolean; stopped: boolean } {
  if (o.status === 'CANCELLED' || o.status === 'REJECTED') {
    const reached = Math.max(0, ...o.events.map(e => STEP_OF[e.status] ?? 0));
    const at = Math.min(reached + 1, STEPS.length - 1);
    return { steps: STEPS.map((s, i) => (i === at ? (o.status === 'CANCELLED' ? '취소' : '진행 불가') : s)), current: at, complete: false, stopped: true };
  }
  return { steps: STEPS, current: STEP_OF[o.status] ?? 0, complete: o.status === 'COMPLETED', stopped: false };
}
export const STATUS_LABEL: Record<OrderStatus, string> = {
  AWAITING_PAYMENT: '결제 대기', SUBMITTED: '접수', IN_PROGRESS: '진행 중', DELIVERED: '결과 전달', COMPLETED: '완료', CANCELLED: '취소', REJECTED: '진행 불가',
};
export const STATUS_TONE: Record<OrderStatus, 'amber' | 'blue' | 'violet' | 'green' | 'gray' | 'red'> = {
  AWAITING_PAYMENT: 'amber', SUBMITTED: 'blue', IN_PROGRESS: 'violet', DELIVERED: 'blue', COMPLETED: 'green', CANCELLED: 'gray', REJECTED: 'red',
};
/** 지금 단계 한 줄 설명 */
export function statusNote(o: AddonOrder): string {
  switch (o.status) {
    case 'AWAITING_PAYMENT': return o.service === 'EXPRESS_RELEASE'
      ? '결제하면 담당자가 먼저 검토해 3영업일 안에 발매되도록 진행해요.'
      : '결제하면 바로 담당자에게 접수돼요.';
    case 'SUBMITTED': return '접수됐어요. 담당자가 시작하기 전까지는 취소하면 바로 환불돼요.';
    case 'IN_PROGRESS': return o.service === 'MV_DISTRIBUTION' && o.tier !== 'BASIC' ? '영상물등급위원회 심의와 배급을 준비하고 있어요.' : '담당자가 작업하고 있어요.';
    case 'DELIVERED': return o.resolution || '결과를 전달했어요. 플랫폼마다 반영까지 조금 걸릴 수 있어요.';
    case 'COMPLETED': return o.resolution || '모두 끝났어요.';
    case 'CANCELLED': return o.refunded_at ? '취소하고 환불했어요.' : '취소했어요.';
    case 'REJECTED': return `${o.resolution || '이번 신청은 진행하지 못했어요.'}${o.paid_at ? ' 결제한 금액은 환불했어요.' : ''}`;
  }
}

/** 가격표 — 무료·유료 모든 부가서비스 (지금 쓸 수 있는 곳으로 바로 가는 주소 포함) */
export interface PriceRow { name: string; price: string; note: string; to?: string; tag?: 'free' | 'paid' | 'soon' }
export const PRICE_TABLE: PriceRow[] = [
  { name: '뮤직비디오 배급·심의', price: '19,000원 / 39,000원 / 49,000원', note: '배급만 · 배급+심의 · 배급+심의+급행', to: '/services?new=mv', tag: 'paid' },
  { name: '신속 발매', price: '10,000원', note: '심사·전송을 먼저 처리해 3영업일 안에 발매', to: '/upload', tag: 'paid' },
  { name: 'AI 가사 싱크 생성', price: '곡당 5,000원', note: '가사에 시간을 맞춘 싱크 가사(LRC)를 만들어 등록', to: '/services?new=lyrics', tag: 'paid' },
  { name: '가사 등록 (LRC)', price: '무료', note: '직접 만든 싱크 가사를 올리면 함께 등록', to: '/services?new=lrc', tag: 'free' },
  { name: 'UGC Rights · Content ID 등록', price: '무료', note: '유튜브 수익 배분 90:10 (아티스트 90)', to: '/inquiries?new=request&group=content_id', tag: 'free' },
  { name: 'Artist Profile Basic', price: '무료', note: '아티스트 페이지·스마트링크·응원', to: '/artist', tag: 'free' },
  { name: 'Audio QC', price: '무료', note: '올릴 때 음원 품질을 자동으로 검사', tag: 'free' },
  { name: 'Dolby Atmos 전달', price: '무료', note: '공간 음향 파일을 지원 플랫폼에 함께 전달', tag: 'free' },
  { name: 'API·피드 기반 자동 DB 등록', price: '무료', note: '음원 정보를 플랫폼·DB에 피드로 자동 등록', tag: 'free' },
  { name: 'Marketing Basic', price: '무료', note: '프로모 이미지·캡션·보도자료', to: '/artist?tab=press', tag: 'free' },
  { name: 'Artist Profile Plus', price: '연 19,000원', note: '곧 열려요', tag: 'soon' },
  { name: 'Marketing Plus', price: '릴리즈당 39,000원', note: '곧 열려요', tag: 'soon' },
];

// ---------------------------------------------------------------------------
// 체험 모드
// ---------------------------------------------------------------------------
const MOCK_PRICES: AddonState['prices'] = { mv: [{ tier: 'BASIC', price: 19000 }, { tier: 'REVIEW', price: 39000 }, { tier: 'EXPRESS', price: 49000 }], express_release: 10000, lyric_sync: 5000 };
const mock = { balance: 52300, items: [] as AddonOrder[] };
const now = () => new Date().toISOString();
const mockEvent = (o: AddonOrder, status: OrderStatus, note = '') => { o.status = status; o.updated_at = now(); o.events = [...o.events, { status, note, by_staff: false, at: now() }]; };
const MOCK_TRACKS: TrackRow[] = [
  { id: 'mt1', title: '여름의 끝', track_number: 1, has_lyrics: true, lrc: null, source: null, registered: [] },
  { id: 'mt2', title: '파도', track_number: 2, has_lyrics: true, lrc: null, source: null, registered: [] },
  { id: 'mt3', title: '여름의 끝 (Inst.)', track_number: 3, has_lyrics: false, lrc: null, source: null, registered: [] },
];

// ---------------------------------------------------------------------------
// 서버
// ---------------------------------------------------------------------------
export async function fetchAddons(): Promise<AddonState> {
  if (MOCK) return { items: [...mock.items], prices: MOCK_PRICES, card: true, balance: mock.balance, mv_platforms: ['melon', 'genie', 'bugs', 'flo', 'vibe', 'youtube'] };
  return req<AddonState>(orgPath('/addons'), { quiet401: true });
}

export interface NewOrder { service: Service; tier?: MvTier; release_id: string; track_ids: string[]; detail: Record<string, unknown> }

export async function createOrder(o: NewOrder, releaseTitle: string, tracks: { id: string; title: string }[]): Promise<AddonOrder> {
  if (MOCK) {
    const unit = o.service === 'LYRIC_SYNC' ? 5000 : MOCK_PRICES.mv.find(t => t.tier === o.tier)?.price ?? 0;
    const order: AddonOrder = {
      id: `mock-${Date.now()}`, service: o.service, tier: o.tier ?? '', release_id: o.release_id, release_title: releaseTitle,
      track_ids: o.track_ids, tracks: tracks.filter(t => o.track_ids.includes(t.id)),
      title: o.service === 'MV_DISTRIBUTION' ? String(o.detail.mv_title ?? '') : `${releaseTitle} 가사 싱크`,
      detail: o.detail, quantity: o.service === 'LYRIC_SYNC' ? o.track_ids.length : 1, unit_price: unit,
      amount: unit * (o.service === 'LYRIC_SYNC' ? o.track_ids.length : 1), status: 'AWAITING_PAYMENT',
      pay_method: null, paid_at: null, refunded_at: null, result: {}, resolution: '', created_at: now(), updated_at: now(),
      events: [{ status: 'AWAITING_PAYMENT', note: '', by_staff: false, at: now() }],
    };
    mock.items = [order, ...mock.items];
    return order;
  }
  return req<AddonOrder>(orgPath('/addons'), { method: 'POST', body: o });
}

export async function paySettlement(id: string): Promise<AddonOrder> {
  if (MOCK) {
    const o = mock.items.find(x => x.id === id)!;
    if (mock.balance < o.amount) throw Object.assign(new Error('정산 잔액이 모자라요.'), { code: 'ADDON_BALANCE_SHORT' });
    mock.balance -= o.amount;
    o.pay_method = 'SETTLEMENT'; o.paid_at = now();
    mockEvent(o, 'SUBMITTED', '정산금에서 결제');
    return { ...o };
  }
  return req<AddonOrder>(orgPath(`/addons/${encodeURIComponent(id)}/pay/settlement`), { method: 'POST', body: {} });
}

export async function cancelOrder(id: string): Promise<AddonOrder> {
  if (MOCK) {
    const o = mock.items.find(x => x.id === id)!;
    if (o.pay_method === 'SETTLEMENT') mock.balance += o.amount;
    if (o.paid_at) o.refunded_at = now();
    mockEvent(o, 'CANCELLED');
    return { ...o };
  }
  return req<AddonOrder>(orgPath(`/addons/${encodeURIComponent(id)}/cancel`), { method: 'POST', body: {} });
}

/** 카드 결제 — 토스 결제창을 열고, 돌아오면 /services?pay=card&id=…에서 confirmCard로 승인한다 */
const PENDING = 'aq-addon-card';
export async function payCard(id: string): Promise<void> {
  if (MOCK) {
    const o = mock.items.find(x => x.id === id)!;
    o.pay_method = 'CARD'; o.paid_at = now();
    mockEvent(o, 'SUBMITTED', '카드 결제');
    return;
  }
  const w = await req<{ client_key: string; customer_key: string; order_id: string; amount: number; order_name: string }>(
    orgPath(`/addons/${encodeURIComponent(id)}/pay/card`), { method: 'POST', body: {} });
  const sdk = await loadToss();
  const back = `${window.location.origin}/services?pay=card&id=${encodeURIComponent(id)}`;
  try { sessionStorage.setItem(PENDING, id); } catch { /* 돌아오는 주소에도 id가 있다 */ }
  const payment = sdk(w.client_key).payment({ customerKey: w.customer_key });
  await payment.requestPayment({
    method: 'CARD', amount: { currency: 'KRW', value: w.amount }, orderId: w.order_id, orderName: w.order_name,
    successUrl: back, failUrl: `${back}&fail=1`,
  });
}

export async function confirmCard(id: string, q: URLSearchParams): Promise<AddonOrder> {
  return req<AddonOrder>(orgPath(`/addons/${encodeURIComponent(id)}/pay/card/confirm`), {
    method: 'POST', body: { payment_key: q.get('paymentKey') ?? '', order_id: q.get('orderId') ?? '', amount: Number(q.get('amount') ?? 0) },
  });
}

type TossFactory = (clientKey: string) => { payment: (o: { customerKey: string }) => { requestPayment: (o: Record<string, unknown>) => Promise<void> } };
let tossLoading: Promise<TossFactory> | null = null;
/** 토스페이먼츠 결제 SDK (v2) — 결제할 때만 불러온다 */
function loadToss(): Promise<TossFactory> {
  tossLoading ??= new Promise((resolve, reject) => {
    const s = document.createElement('script');
    s.src = 'https://js.tosspayments.com/v2/standard';
    s.async = true;
    s.onload = () => {
      const f = (window as unknown as { TossPayments?: TossFactory }).TossPayments;
      if (f) resolve(f); else reject(new Error('결제창을 불러오지 못했어요.'));
    };
    s.onerror = () => { tossLoading = null; reject(new Error('결제창을 불러오지 못했어요. 잠시 후 다시 시도해 주세요.')); };
    document.head.appendChild(s);
  });
  return tossLoading;
}

export async function fetchTracks(releaseId: string): Promise<TrackRow[]> {
  if (MOCK) return MOCK_TRACKS.map(t => ({ ...t }));
  return (await req<{ items: TrackRow[] }>(orgPath(`/releases/${encodeURIComponent(releaseId)}/lyric-sync`), { quiet401: true })).items;
}

export async function putLrc(releaseId: string, trackId: string, lrc: string): Promise<number> {
  if (MOCK) {
    const n = lrcLines(lrc);
    if (n === null) throw Object.assign(new Error('싱크 가사 형식이 아니에요.'), { code: 'LRC_INVALID' });
    const t = MOCK_TRACKS.find(x => x.id === trackId);
    if (t) { t.lrc = lrc; t.source = 'ARTIST'; }
    return n;
  }
  return (await req<{ lines: number }>(orgPath(`/releases/${encodeURIComponent(releaseId)}/tracks/${encodeURIComponent(trackId)}/lyric-sync`), { method: 'PUT', body: { lrc } })).lines;
}

/** 서버와 같은 LRC 검사 — 시간이 붙은 줄 수, 형식이 아니면 null */
export function lrcLines(lrc: string): number | null {
  if (!lrc.trim() || lrc.length > 60000) return null;
  let timed = 0;
  for (const raw of lrc.split(/\r?\n/)) {
    let rest = raw.trim();
    if (!rest) continue;
    let stamps = 0;
    while (rest.startsWith('[')) {
      const end = rest.indexOf(']');
      if (end < 0) return null;
      const tag = rest.slice(1, end);
      const m = /^(\d{1,3}):(\d{2})(?:[.:](\d{0,3}))?$/.exec(tag);
      if (m) { if (Number(m[2]) >= 60) return null; stamps++; }
      else if (!/^[A-Za-z]+:/.test(tag)) return null;
      rest = rest.slice(end + 1);
    }
    if (stamps > 0) timed++;
    else if (rest) return null;
  }
  return timed || null;
}
