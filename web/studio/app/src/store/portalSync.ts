// 실서버 모드 동기화 — 로그인하면 포털 데이터(프로필·계좌·알림·문의·서류·정산)를 서버에서 채우고,
// 알림은 1분마다·창으로 돌아올 때 새로 받는다. 체험 모드에서는 쓰지 않는다.
import * as portal from '../api/portal';
import { setProfile } from './profile';
import { setPayment } from './payment';
import { noticesStore } from './support';
import { ticketsStore } from './tickets';
import { setDocs } from './docs';
import { financeStore, payoutsStore, statementsStore } from './settlement';

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
  statementsStore.set(f.statements);
  payoutsStore.set(f.payouts);
  financeStore.set({ ...f.summary, loaded: true });
}
export async function refreshPayment() {
  const p = await quiet(portal.fetchPayoutAccount());
  if (p !== undefined) setPayment(p);
}

export async function hydratePortal() {
  await Promise.all([
    quiet(portal.fetchProfile()).then(p => { if (p) setProfile(p); }),
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
  noticesStore.set([]);
  ticketsStore.set([]);
  setDocs([]);
  statementsStore.set([]);
  payoutsStore.set([]);
  financeStore.set({ payable: 0, pending: 0, available: 0, minimum: 10000, loaded: false });
}

export function startPortalSync(): () => void {
  void hydratePortal();
  const timer = window.setInterval(() => { if (document.visibilityState === 'visible') void refreshNotifications(); }, 60_000);
  const onFocus = () => { void refreshNotifications(); };
  const onRefresh = () => { void Promise.all([refreshNotifications(), refreshDocs(), refreshFinance(), refreshTickets()]); };
  window.addEventListener('focus', onFocus);
  window.addEventListener(PORTAL_REFRESH_EVENT, onRefresh);
  return () => {
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
