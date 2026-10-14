// 실서버 모드 동기화 — 로그인하면 포털 데이터(프로필·계정 정보·계좌·알림·문의·서류·정산)를 서버에서 채우고,
// 알림·문의는 실시간 연결(SSE)로 바뀌는 즉시 다시 받는다. 연결이 안 되면 짧은 주기로 새로 고친다.
// 체험 모드에서는 쓰지 않는다.
import * as portal from '../api/portal';
import { orgPath } from '../api/http';
import { openLive } from '../lib/live';
import { systemAlert } from '../lib/alerts';
import { INQUIRY_CHANGED_EVENT, LIVE_NOTICE_EVENT } from '../lib/liveEvents';
import { setProfile } from './profile';
import { setPayment } from './payment';
import { EMPTY_ACCOUNT, setAccount } from './account';
import { noticesStore } from './support';
import { ticketsStore } from './tickets';
import { setDocs } from './docs';
import { financeStore, payoutsStore, statementsNextStore, statementsStore } from './settlement';

export const PORTAL_REFRESH_EVENT = 'aq:portal-refresh';


const quiet = <T,>(p: Promise<T>) => p.catch(err => { console.warn('[portal]', err); return undefined; });

export async function refreshNotifications() {
  const n = await quiet(portal.fetchNotifications());
  if (n) noticesStore.set(n);
}
export async function refreshTickets() {
  const t = await quiet(portal.fetchInquiries());
  if (t) ticketsStore.set(t);
}
export async function refreshDocs() {
  const d = await quiet(portal.fetchDocuments());
  if (d) setDocs(d);
}
export async function refreshFinance() {
  const f = await quiet(portal.fetchFinance());
  if (!f) return;
  // '더 보기'로 받아 둔 이전 쪽은 주기적 새로고침에도 남긴다 (첫 쪽만 새로 받는다)
  const loadedMore = statementsNextStore.get() !== f.statementsNext && statementsStore.get().length > f.statements.length;
  if (loadedMore) {
    const fresh = new Set(f.statements.map(x => x.id));
    statementsStore.set(old => [...f.statements, ...old.filter(x => !fresh.has(x.id))]);
  } else {
    statementsStore.set(f.statements);
    statementsNextStore.set(f.statementsNext);
  }
  payoutsStore.set(f.payouts);
  financeStore.set({ ...f.summary, loaded: true });
}
export async function refreshPayment() {
  const p = await quiet(portal.fetchPayoutAccount());
  if (p !== undefined) setPayment(p);
}

export async function refreshAccount() {
  const a = await quiet(portal.fetchAccountInfo());
  if (a) setAccount(a);
}

export async function hydratePortal() {
  await Promise.all([
    quiet(portal.fetchProfile()).then(p => { if (p) setProfile(p); }),
    refreshAccount(),
    refreshPayment(),
    refreshNotifications(),
    refreshTickets(),
    refreshDocs(),
    refreshFinance(),
  ]);
}

/** 로그아웃·계정 전환 시 이전 계정 데이터를 비운다 */
export function clearPortal() {
  setProfile({ name: '', email: '', bio: '', country: 'KR' });
  setPayment(null);
  setAccount(EMPTY_ACCOUNT);
  noticesStore.set([]);
  ticketsStore.set([]);
  setDocs([]);
  statementsStore.set([]);
  statementsNextStore.set(null);
  payoutsStore.set([]);
  financeStore.set({ payable: 0, pending: 0, available: 0, held: 0, minimum: 10000, loaded: false });
}

/** 알림을 새로 받고 처음 보는 읽지 않은 알림을 돌려준다.
 *  새 알림은 대개 담당자 처리(심사 승인 → 계약서 서명 가능, 서류 요청·승인)라서 서류 목록도 새로 받는다.
 *  서류를 로그인할 때만 받으면 승인 뒤에도 계약서가 ‘검토 중’으로 남아 서명할 수 없다. */
async function refreshNotificationsAndDocs() {
  const before = new Set(noticesStore.get().map(n => n.id));
  await refreshNotifications();
  const fresh = noticesStore.get().filter(n => !n.read && !before.has(n.id));
  if (fresh.length) void refreshDocs();
  return fresh;
}

/** 새로 받은 알림 중 처음 보는 읽지 않은 것을 알린다 — 다른 탭이면 운영체제 알림, 보고 있으면 토스트 */
async function refreshNotificationsAndAlert() {
  const fresh = await refreshNotificationsAndDocs();
  for (const n of fresh.slice(0, 3)) {
    if (!systemAlert(n.title, n.detail, n.link, `notice-${n.id}`)) {
      window.dispatchEvent(new CustomEvent(LIVE_NOTICE_EVENT, { detail: n }));
    }
  }
}

export function startPortalSync(): () => void {
  void hydratePortal();
  let path = '';
  try { path = orgPath('/live'); } catch { /* 작업 공간 없음 — 실시간 연결 없이 주기 새로 고침만 */ }
  const stopLive = path ? openLive(path, {
    on: {
      notifications: d => { if (d.new) void refreshNotificationsAndAlert(); else void refreshNotifications(); },
      inquiries: d => {
        void refreshTickets();
        window.dispatchEvent(new CustomEvent(INQUIRY_CHANGED_EVENT, { detail: { inquiryId: d.inquiry_id ?? null } }));
      },
    },
    // 다시 연결되면 놓친 담당자 처리가 있을 수 있어 서류도 새로 받는다
    resync: () => { void refreshNotifications(); void refreshTickets(); void refreshDocs(); },
  }) : () => {};
  // 탭·앱을 자주 오가도 알림 요청은 15초에 한 번만 (Worker·API 호출 절약)
  let last = Date.now();
  const refresh = () => {
    if (Date.now() - last < 15_000) return;
    last = Date.now();
    void refreshNotificationsAndDocs();
  };
  const timer = window.setInterval(() => { if (document.visibilityState === 'visible') refresh(); }, 60_000);
  const onFocus = refresh;
  const onRefresh = () => { void Promise.all([refreshNotifications(), refreshDocs(), refreshFinance(), refreshTickets()]); };
  window.addEventListener('focus', onFocus);
  window.addEventListener(PORTAL_REFRESH_EVENT, onRefresh);
  return () => {
    stopLive();
    window.clearInterval(timer);
    window.removeEventListener('focus', onFocus);
    window.removeEventListener(PORTAL_REFRESH_EVENT, onRefresh);
    clearPortal();
  };
}

/** 서버 작업 뒤 관련 화면 데이터를 다시 받게 알린다 */
export function requestPortalRefresh() {
  window.dispatchEvent(new Event(PORTAL_REFRESH_EVENT));
}
