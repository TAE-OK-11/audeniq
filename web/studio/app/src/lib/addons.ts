// 부가서비스 — 서버 crates/core/src/addons.rs, payments.rs. 발매 신청서의 '부가서비스' 칸에서 고르면 접수할 때 주문이 생기고,
// 신청 마지막(신청 완료 화면)이나 발매 상세에서 한 번에 결제한다: 정산금(정산 잔액에서 바로 차감) 또는 카드(설정한 PG —
// 토스페이먼츠·포트원·페이플). 체험 모드는 같은 모양을 브라우저 안에서 흉내 낸다.
import { orgPath, req } from '../api/http';
import { uploadParts } from '../api/multipart';
import { MOCK } from './mode';

export type Service = 'MV_DISTRIBUTION' | 'EXPRESS_RELEASE' | 'LYRIC_SYNC';
export type MvTier = 'BASIC' | 'REVIEW' | 'EXPRESS';
export type OrderStatus = 'AWAITING_PAYMENT' | 'SUBMITTED' | 'IN_PROGRESS' | 'DELIVERED' | 'COMPLETED' | 'CANCELLED' | 'REJECTED';

export interface AddonOrder {
  id: string; service: Service; tier: '' | MvTier;
  release_id: string; release_title: string | null;
  track_ids: string[]; tracks: { id: string; title: string }[];
  video: { id: string; name: string; size_bytes: number } | null;
  title: string; detail: Record<string, unknown>;
  quantity: number; unit_price: number; amount: number;
  status: OrderStatus; pay_method: 'SETTLEMENT' | 'CARD' | null; paid_at: string | null; refunded_at: string | null;
  provider: string | null;
  result: { rating?: string; rating_no?: string; note?: string; links?: Record<string, string> };
  resolution: string; created_at: string; updated_at: string;
  events: { status: string; note: string; by_staff: boolean; at: string }[];
}

export interface Prices { mv: { tier: MvTier; price: number }[]; express_release: number; lyric_sync: number }
export interface CardInfo { provider: 'toss' | 'portone' | 'payple'; label: string }
export interface AddonConfig { prices: Prices; card: CardInfo | null; balance: number; mv_platforms: string[] }
export interface ReleaseAddons { items: AddonOrder[]; prices: Prices; card: CardInfo | null; balance: number }

export interface TrackRow { id: string; title: string; track_number: number; has_lyrics: boolean; lrc: string | null; source: 'ARTIST' | 'AI' | null; registered: string[] }

/** 서버와 같은 가격 (부가세 포함) — 신청서에서 미리 보여 줄 때 */
export const PRICES: Prices = {
  mv: [{ tier: 'BASIC', price: 19000 }, { tier: 'REVIEW', price: 39000 }, { tier: 'EXPRESS', price: 49000 }],
  express_release: 10000, lyric_sync: 5000,
};
export const MV_PLATFORMS = ['melon', 'genie', 'bugs', 'flo', 'vibe', 'youtube'];

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

/** 진행 단계 카드 — 결제 → 접수 → 진행 중 → 결과 전달 → 완료 (취소·진행 불가는 멈춘 자리) */
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

// ---------------------------------------------------------------------------
// 체험 모드
// ---------------------------------------------------------------------------
const now = () => new Date().toISOString();
/** 체험 모드 예시 — '여름 EP'(r2)에 결제를 기다리는 뮤직비디오 배급과 끝난 가사 싱크 */
const seed = (id: string, service: Service, tier: '' | MvTier, amount: number, qty: number, title: string, status: OrderStatus, extra: Partial<AddonOrder> = {}): AddonOrder => ({
  id, service, tier, release_id: 'r2', release_title: '여름 EP', track_ids: [], tracks: [], video: null, title, detail: {},
  quantity: qty, unit_price: amount / qty, amount, status, pay_method: null, paid_at: null, refunded_at: null, provider: null,
  result: {}, resolution: '', created_at: '2026-09-22T09:10:00+09:00', updated_at: '2026-09-22T09:10:00+09:00',
  events: [{ status: 'AWAITING_PAYMENT', note: '', by_staff: false, at: '2026-09-22T09:10:00+09:00' }], ...extra,
});
const mock = {
  balance: 52300,
  orders: [
    seed('mock-mv', 'MV_DISTRIBUTION', 'REVIEW', 39000, 1, '여름 EP 타이틀 MV', 'AWAITING_PAYMENT', { detail: { mv_title: '여름 EP 타이틀 MV', duration: '3:45', platforms: ['melon', 'genie', 'youtube'] } }),
    seed('mock-lyric', 'LYRIC_SYNC', '', 10000, 2, '여름 EP 가사 싱크', 'COMPLETED', {
      pay_method: 'SETTLEMENT', paid_at: '2026-09-22T09:12:00+09:00', resolution: 'Musixmatch·멜론·지니에 등록했어요.',
      tracks: [{ id: 'mt1', title: '여름의 끝' }, { id: 'mt2', title: '파도' }],
      events: [
        { status: 'AWAITING_PAYMENT', note: '', by_staff: false, at: '2026-09-22T09:10:00+09:00' },
        { status: 'SUBMITTED', note: '정산금에서 결제', by_staff: false, at: '2026-09-22T09:12:00+09:00' },
        { status: 'COMPLETED', note: '', by_staff: true, at: '2026-09-24T15:00:00+09:00' },
      ],
    }),
  ] as AddonOrder[],
};
const MOCK_TRACKS: TrackRow[] = [
  { id: 'mt1', title: '여름의 끝', track_number: 1, has_lyrics: true, lrc: null, source: null, registered: [] },
  { id: 'mt2', title: '파도', track_number: 2, has_lyrics: true, lrc: null, source: null, registered: [] },
];
const mockState = (rid: string): ReleaseAddons => ({ items: mock.orders.filter(o => o.release_id === rid).map(o => ({ ...o })), prices: PRICES, card: { provider: 'toss', label: '토스페이먼츠' }, balance: mock.balance });
const mockStep = (o: AddonOrder, status: OrderStatus, note = '') => { o.status = status; o.updated_at = now(); o.events = [...o.events, { status, note, by_staff: false, at: now() }]; };

/** 체험 모드 — 신청서 옵션으로 주문을 만든다 (실서버는 접수할 때 서버가 만든다) */
export function mockOrdersFromApplication(releaseId: string, title: string, o: Record<string, unknown>, trackTitles: string[]) {
  if (!MOCK) return;
  const make = (service: Service, tier: '' | MvTier, unit: number, qty: number, name: string, tracks: string[]) => {
    if (mock.orders.some(x => x.release_id === releaseId && x.service === service && !['CANCELLED', 'REJECTED'].includes(x.status))) return;
    mock.orders.push({
      id: `mock-${service}-${Date.now()}`, service, tier, release_id: releaseId, release_title: title, track_ids: tracks,
      tracks: tracks.map((t, i) => ({ id: t, title: trackTitles[i] ?? t })), video: null, title: name, detail: {},
      quantity: qty, unit_price: unit, amount: unit * qty, status: 'AWAITING_PAYMENT', pay_method: null, paid_at: null,
      refunded_at: null, provider: null, result: {}, resolution: '', created_at: now(), updated_at: now(),
      events: [{ status: 'AWAITING_PAYMENT', note: '', by_staff: false, at: now() }],
    });
  };
  if (o.express) make('EXPRESS_RELEASE', '', 10000, 1, title, []);
  if (o.mv) make('MV_DISTRIBUTION', (o.mvTier as MvTier) || 'REVIEW', PRICES.mv.find(t => t.tier === o.mvTier)?.price ?? 39000, 1, String(o.mvTitle || `${title} MV`), []);
  if (o.lyricSync) {
    const n = Array.isArray(o.lyricTracks) ? o.lyricTracks.length : 0;
    if (n) make('LYRIC_SYNC', '', 5000, n, `${title} 가사 싱크`, (o.lyricTracks as number[]).map(String));
  }
}

// ---------------------------------------------------------------------------
// 서버
// ---------------------------------------------------------------------------
export async function fetchConfig(): Promise<AddonConfig> {
  if (MOCK) return { prices: PRICES, card: { provider: 'toss', label: '토스페이먼츠' }, balance: mock.balance, mv_platforms: MV_PLATFORMS };
  return req<AddonConfig>(orgPath('/addons/config'), { quiet401: true });
}

export async function fetchReleaseAddons(releaseId: string): Promise<ReleaseAddons> {
  if (MOCK) return mockState(releaseId);
  return req<ReleaseAddons>(orgPath(`/releases/${encodeURIComponent(releaseId)}/addons`), { quiet401: true });
}

export async function payWithBalance(releaseId: string, orderIds: string[]): Promise<ReleaseAddons> {
  if (MOCK) {
    const list = mock.orders.filter(o => orderIds.includes(o.id) && o.status === 'AWAITING_PAYMENT');
    const total = list.reduce((n, o) => n + o.amount, 0);
    if (mock.balance < total) throw Object.assign(new Error('출금할 수 있는 정산금이 모자라요.'), { code: 'ADDON_BALANCE_SHORT' });
    mock.balance -= total;
    for (const o of list) { o.pay_method = 'SETTLEMENT'; o.paid_at = now(); mockStep(o, 'SUBMITTED', '정산금에서 결제'); }
    return mockState(releaseId);
  }
  return req<ReleaseAddons>(orgPath(`/releases/${encodeURIComponent(releaseId)}/addons/pay`), { method: 'POST', body: { method: 'SETTLEMENT', order_ids: orderIds } });
}

export async function cancelOrder(id: string, releaseId: string): Promise<ReleaseAddons> {
  if (MOCK) {
    const o = mock.orders.find(x => x.id === id)!;
    if (o.pay_method === 'SETTLEMENT') mock.balance += o.amount;
    if (o.paid_at) o.refunded_at = now();
    mockStep(o, 'CANCELLED');
    return mockState(releaseId);
  }
  return req<ReleaseAddons>(orgPath(`/addons/${encodeURIComponent(id)}/cancel`), { method: 'POST', body: {} });
}

// ---------------------------------------------------------------------------
// 카드 결제 — PG마다 결제창이 다르다. 결제창이 돌아오면 서버가 금액을 다시 확인하고 승인한다.
// ---------------------------------------------------------------------------
type Window = Record<string, unknown> & { provider: CardInfo['provider']; payment_id: string; amount: number; order_name: string };

/** 결제창에서 돌아올 주소 — 발매 상세가 ?pay=…&payment=…를 읽어 승인을 마친다 */
const backUrl = (releaseId: string, provider: string, payment: string) =>
  `${window.location.origin}/releases/${encodeURIComponent(releaseId)}?pay=${provider}&payment=${encodeURIComponent(payment)}`;

/** 카드로 결제 — 결제창을 연다. 바로 결과가 나오는 결제창(PC 포트원·페이플)이면 승인까지 마치고 새 상태를 돌려주고,
 *  다른 페이지로 넘어가는 결제창이면 돌아온 뒤 발매 상세에서 마친다 (그때는 null) */
export async function payWithCard(releaseId: string, orderIds: string[]): Promise<ReleaseAddons | null> {
  if (MOCK) {
    for (const o of mock.orders.filter(x => orderIds.includes(x.id) && x.status === 'AWAITING_PAYMENT')) {
      o.pay_method = 'CARD'; o.paid_at = now(); o.provider = 'toss'; mockStep(o, 'SUBMITTED', '카드 결제');
    }
    return mockState(releaseId);
  }
  const w = await req<Window>(orgPath(`/releases/${encodeURIComponent(releaseId)}/addons/pay`), { method: 'POST', body: { method: 'CARD', order_ids: orderIds } });
  const back = backUrl(releaseId, w.provider, w.payment_id);
  if (w.provider === 'toss') {
    await loadScript('https://js.tosspayments.com/v2/standard');
    const toss = (window as unknown as { TossPayments: (k: string) => { payment: (o: { customerKey: string }) => { requestPayment: (o: Record<string, unknown>) => Promise<void> } } }).TossPayments;
    await toss(String(w.client_key)).payment({ customerKey: String(w.customer_key) }).requestPayment({
      method: 'CARD', amount: { currency: 'KRW', value: w.amount }, orderId: String(w.order_id), orderName: w.order_name,
      successUrl: back, failUrl: `${back}&fail=1`,
    });
    return null;
  }
  if (w.provider === 'portone') {
    await loadScript('https://cdn.portone.io/v2/browser-sdk.js');
    const portone = (window as unknown as { PortOne: { requestPayment: (o: Record<string, unknown>) => Promise<{ code?: string; message?: string } | undefined> } }).PortOne;
    const r = await portone.requestPayment({
      storeId: w.store_id, channelKey: w.channel_key, paymentId: w.pg_payment_id,
      orderName: w.order_name, totalAmount: w.amount, currency: 'CURRENCY_KRW', payMethod: 'CARD', redirectUrl: back,
    });
    if (!r) return null; // 리다이렉트로 넘어갔다
    if (r.code) throw Object.assign(new Error(r.message || '결제를 마치지 못했어요.'), { code: 'ADDON_CARD_FAILED' });
    return confirmPayment(w.payment_id, {});
  }
  // 페이플 — jQuery가 먼저 있어야 한다. 결과는 callbackFunction으로 받는다 (PCD_RST_URL은 상대 경로).
  await loadScript('https://ajax.googleapis.com/ajax/libs/jquery/3.4.1/jquery.min.js');
  await loadScript(String(w.script));
  return new Promise((resolve, reject) => {
    const check = (window as unknown as { PaypleCpayAuthCheck: (o: Record<string, unknown>) => void }).PaypleCpayAuthCheck;
    check({
      clientKey: w.client_key, PCD_PAY_TYPE: 'card', PCD_PAY_WORK: 'CERT', PCD_CARD_VER: '02',
      PCD_PAY_GOODS: w.order_name, PCD_PAY_TOTAL: w.amount, PCD_PAY_OID: w.order_id, PCD_RST_URL: w.result_url,
      callbackFunction: (res: Record<string, string>) => {
        if (res.PCD_PAY_RST !== 'success') {
          reject(Object.assign(new Error(res.PCD_PAY_RST === 'close' ? '결제를 닫았어요.' : res.PCD_PAY_MSG || '결제를 마치지 못했어요.'), { code: 'ADDON_CARD_FAILED' }));
          return;
        }
        confirmPayment(w.payment_id, { auth_key: res.PCD_AUTH_KEY, req_key: res.PCD_PAY_REQKEY, order_id: res.PCD_PAY_OID, total: String(res.PCD_PAY_TOTAL) })
          .then(resolve, reject);
      },
    });
  });
}

export async function confirmPayment(paymentId: string, input: Record<string, unknown>): Promise<ReleaseAddons> {
  return req<ReleaseAddons>(orgPath(`/addon-payments/${encodeURIComponent(paymentId)}/confirm`), { method: 'POST', body: input });
}

/** 결제창에서 돌아온 주소(?pay=toss|portone&payment=…)의 결과로 승인 — 실패로 돌아왔으면 그 메시지로 에러 */
export async function confirmFromReturn(q: URLSearchParams): Promise<ReleaseAddons> {
  const payment = q.get('payment') ?? '';
  if (q.get('fail') || q.get('code')) throw Object.assign(new Error(q.get('message') || '결제를 마치지 못했어요.'), { code: 'ADDON_CARD_FAILED' });
  return confirmPayment(payment, q.get('pay') === 'toss' ? { payment_key: q.get('paymentKey') ?? '' } : {});
}

const loading = new Map<string, Promise<void>>();
function loadScript(src: string): Promise<void> {
  let p = loading.get(src);
  if (!p) {
    p = new Promise((resolve, reject) => {
      const s = document.createElement('script');
      s.src = src;
      s.async = true;
      s.onload = () => resolve();
      s.onerror = () => { loading.delete(src); reject(new Error('결제창을 불러오지 못했어요. 잠시 후 다시 시도해 주세요.')); };
      document.head.appendChild(s);
    });
    loading.set(src, p);
  }
  return p;
}

// ---------------------------------------------------------------------------
// 뮤직비디오 파일 — 16 MiB 조각으로 저장소에 바로 올린다 (끊기면 이어서)
// ---------------------------------------------------------------------------
export const MAX_VIDEO_BYTES = 8 * 1024 ** 3;
/** 올린 영상 — 길이(m:ss)와 화면 크기는 서버가 파일 머리에서 읽은 값 (없으면 '' · 0) */
export interface UploadedVideo { id: string; duration: string; width: number; height: number }

/** 모의 모드: 서버 대신 브라우저가 영상 머리를 읽는다 */
function readVideoMeta(file: File): Promise<Omit<UploadedVideo, 'id'>> {
  return new Promise(resolve => {
    const v = document.createElement('video');
    const url = URL.createObjectURL(file);
    const done = (d: Omit<UploadedVideo, 'id'>) => { URL.revokeObjectURL(url); resolve(d); };
    v.preload = 'metadata';
    v.onloadedmetadata = () => {
      const s = Number.isFinite(v.duration) ? Math.floor(v.duration) : 0;
      done({ duration: s ? `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}` : '', width: v.videoWidth, height: v.videoHeight });
    };
    v.onerror = () => done({ duration: '', width: 0, height: 0 });
    v.src = url;
  });
}

export async function uploadVideo(file: File, onProgress?: (r: number) => void, signal?: AbortSignal): Promise<UploadedVideo> {
  const type = file.type === 'video/quicktime' || /\.mov$/i.test(file.name) ? 'video/quicktime' : 'video/mp4';
  if (MOCK) {
    for (let i = 1; i <= 10; i++) { await new Promise(r => setTimeout(r, 60)); onProgress?.(i / 10); }
    return { id: `mock-video-${Date.now()}`, ...(await readVideoMeta(file)) };
  }
  const v = await req<{ id: string }>(orgPath('/addon-videos'), { method: 'POST', body: { name: file.name, size_bytes: file.size, content_type: type } });
  await uploadParts(file, v.id, onProgress, signal, `/addon-videos/${encodeURIComponent(v.id)}/parts`);
  return req<UploadedVideo>(orgPath(`/addon-videos/${encodeURIComponent(v.id)}/complete`), { method: 'POST', body: {} });
}

// ---------------------------------------------------------------------------
// 싱크 가사 (LRC)
// ---------------------------------------------------------------------------
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
