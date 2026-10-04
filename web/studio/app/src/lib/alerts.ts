// 브라우저 알림 — 스튜디오(아티스트)와 관리자(스태프)가 같이 쓴다.
// 화면을 보고 있으면 앱 안 토스트로, 다른 탭·창에 있으면 운영체제 알림으로 알린다.
// 사용자가 켠 경우에만(권한 허용 + 설정 켬) 보낸다. 내용은 짧게, 누르면 해당 화면이 열린다.
import { toHref } from './router';

const KEY = 'aq.browserAlerts';

export const alertsSupported = () => typeof window !== 'undefined' && 'Notification' in window;

function stored(): boolean {
  try { return localStorage.getItem(KEY) === 'on'; } catch { return false; }
}

export function alertsEnabled(): boolean {
  return alertsSupported() && stored() && Notification.permission === 'granted';
}

export function alertsBlocked(): boolean {
  return alertsSupported() && Notification.permission === 'denied';
}

export async function enableAlerts(): Promise<boolean> {
  if (!alertsSupported()) return false;
  const p = Notification.permission === 'default' ? await Notification.requestPermission() : Notification.permission;
  try { localStorage.setItem(KEY, p === 'granted' ? 'on' : 'off'); } catch { /* 저장소 차단 — 이번 창에서만 */ }
  return p === 'granted';
}

export function disableAlerts() {
  try { localStorage.setItem(KEY, 'off'); } catch { /* 저장소 차단 */ }
}

/** 앱 안에서 주소 이동 (라우터 밖에서 부를 때) */
export function goTo(link: string) {
  const target = toHref(link);
  if (window.location.pathname + window.location.search === target) return;
  window.history.pushState({}, '', target);
  window.dispatchEvent(new PopStateEvent('popstate'));
}

/** 다른 탭·창에 있을 때만 운영체제 알림. 보내면 true (화면을 보고 있으면 false → 호출한 쪽이 토스트로) */
export function systemAlert(title: string, body: string, link?: string, tag?: string): boolean {
  if (!alertsEnabled()) return false;
  if (document.visibilityState === 'visible' && document.hasFocus()) return false;
  try {
    const n = new Notification(title, { body, tag, icon: `${import.meta.env.BASE_URL}static/favicon.svg` });
    n.onclick = () => {
      window.focus();
      if (link) goTo(link);
      n.close();
    };
    return true;
  } catch {
    // 일부 모바일 브라우저는 페이지에서 직접 알림을 만들 수 없다 (서비스 워커 필요) — 토스트로 대신
    return false;
  }
}
