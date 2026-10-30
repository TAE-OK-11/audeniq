import { loginRedirect } from '../lib/loginRedirect';
// 서비스·기기 상태 알림
// - 서버 점검: /api/status(백엔드·비상 스위치)에 진행 중인 점검이 있거나 API가 503 MAINTENANCE로 답하면
//   점검 화면을 앱 위에 덮는다 (앱은 그대로 두어 작성 중인 내용이 남는다). 72시간 안의 점검은 상단 예고
// - 서버 장애: API가 500/502/503/504·연결 실패면 닫기 없는 전체 화면 점검으로 덮는다
//   (2026-10-28 수정: 닫을 수 있는 모달은 닫으면 뒤 로그인 화면이 보여서, 점검 화면과
//   같은 전체 화면으로 바꿨다. 앱은 inert로 가리고 언마운트하지 않으므로, 서버가
//   복구되면 작성 중인 입력은 그대로 이어서 쓸 수 있다. 관리 화면은 점검과 같이 제외한다)
// - 새 버전: 배포로 index의 앱 번들이 바뀌면 새로고침 안내 창
// - 기기 문제: 오프라인 배너, 쿠키·저장소 차단, 보안 연결이 아닌 경우 안내 창
import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { Modal, useModalClose } from './Modal';
import { ErrorIcon, ErrorScreen, incidentMeta, type ErrorKind } from './ErrorScreen';
import { useToast } from './Toast';
import { useLocation } from '../lib/router';
import { SERVER_ISSUE_EVENT, type ServerIssue } from '../lib/systemEvents';
import { fetchStatus, type MaintenanceWindow } from '../api/content';
import { Glyph } from './Glyph';

// 긴급 점검을 1분 안에 알아차리도록 (백엔드의 /api/status는 가벼운 상태 읽기다)
const STATUS_EVERY_MS = 60_000;
// 점검·서버 장애 중에는 같은 간격으로 상태를 확인해, 복구되면 전체 화면을 자동으로 내린다.
const STATUS_DURING_SERVER_DOWN_MS = 30_000;
const VERSION_EVERY_MS = 5 * 60_000;
const VERSION_MIN_GAP_MS = 60_000;
/** ‘지금 새로고침’을 누른 대상 버전 — 새로고침 뒤에도 같은 차이면 다시 묻지 않는다 */
const RELOADED_FOR_KEY = 'aq-update-reloaded-for';

const DISMISS_KEY = 'aq.maintenance-dismissed';
const DEVICE_KEY = 'aq.device-checked';
const safeGet = (store: 'local' | 'session', k: string) => {
  try { return (store === 'local' ? localStorage : sessionStorage).getItem(k); } catch { return null; }
};
const safeSet = (store: 'local' | 'session', k: string, v: string) => {
  try { (store === 'local' ? localStorage : sessionStorage).setItem(k, v); } catch { /* 저장 불가 */ }
};

/** 2026-09-30T17:00:00Z → '9월 30일(수) 02:00' (한국 시간) */
export function kstWhen(iso: string, withDate = true): string {
  const d = new Date(Date.parse(iso) + 9 * 3_600_000);
  const hm = `${String(d.getUTCHours()).padStart(2, '0')}:${String(d.getUTCMinutes()).padStart(2, '0')}`;
  if (!withDate) return hm;
  const wd = '일월화수목금토'[d.getUTCDay()];
  return `${d.getUTCMonth() + 1}월 ${d.getUTCDate()}일(${wd}) ${hm}`;
}

/** 점검 기간 문구 — 같은 날이면 끝 시각만, 끝을 모르면 '종료 시각 미정' */
export function windowLabel(w: Pick<MaintenanceWindow, 'starts_at' | 'ends_at' | 'end_unknown'>): string {
  if (w.end_unknown) return `${kstWhen(w.starts_at)}부터 · 종료 시각 미정`;
  const sameDay = kstWhen(w.starts_at).slice(0, -6) === kstWhen(w.ends_at).slice(0, -6);
  return `${kstWhen(w.starts_at)} ~ ${sameDay ? kstWhen(w.ends_at, false) : kstWhen(w.ends_at)}`;
}

// ---------------------------------------------------------------------------
// 새 버전 감지
// ---------------------------------------------------------------------------
const ENTRY_RE = /src="([^"]*\/assets\/index-[\w-]+\.js)"/;
function currentEntry(): string | null {
  const el = document.querySelector<HTMLScriptElement>('script[type="module"][src*="/assets/index-"]');
  return el ? new URL(el.src, window.location.href).pathname : null;
}
async function latestEntry(timeoutMs = 10_000): Promise<string | null> {
  try {
    // 주소마다 다른 쿼리를 붙여 CDN(CloudFront) 캐시를 건너뛴다. '/'는 엣지에 캐시돼 연달아 배포한 뒤
    // 한동안 옛 index를 돌려줬다 — 새 버전 화면에서도 '다른 버전'으로 보여 새로고침해도 이 창이 계속 떴다
    const res = await fetch(`${import.meta.env.BASE_URL}?__v=${Date.now()}`, { cache: 'no-store', credentials: 'same-origin', headers: { Accept: 'text/html' }, signal: AbortSignal.timeout(timeoutMs) });
    if (!res.ok) return null;
    const m = ENTRY_RE.exec(await res.text());
    return m ? new URL(m[1], window.location.href).pathname : null;
  } catch {
    return null;
  }
}

/** 새 버전으로 다시 연다 — 같은 주소를 reload()하면 엣지(CloudFront)·브라우저가 캐시해 둔 옛 index를 다시 줘서
 *  '지금 새로고침'을 눌러도 그대로였다. 주소마다 다른 표시(__r)를 붙여 새 index를 받고, 열리면 main.tsx가 표시를 지운다 */
function reloadFresh() {
  const url = new URL(window.location.href);
  url.searchParams.set('__r', Date.now().toString(36));
  window.location.replace(url.toString());
}

// ---------------------------------------------------------------------------
// 기기 점검
// ---------------------------------------------------------------------------
interface DeviceIssue { code: string; title: string; body: string }
export function detectDeviceIssues(): DeviceIssue[] {
  const out: DeviceIssue[] = [];
  if (!navigator.cookieEnabled) {
    out.push({ code: 'COOKIES_DISABLED', title: '쿠키가 꺼져 있어요', body: '로그인 상태를 유지하려면 브라우저 설정에서 이 사이트의 쿠키를 허용해 주세요.' });
  }
  try {
    const k = '__aq_probe__';
    localStorage.setItem(k, '1');
    localStorage.removeItem(k);
  } catch {
    out.push({ code: 'STORAGE_BLOCKED', title: '브라우저 저장소를 쓸 수 없어요', body: '사생활 보호 모드이거나 저장 공간이 가득 찼을 수 있어요. 작성 중인 내용이 새로고침 후 사라질 수 있으니 일반 창에서 이용해 주세요.' });
  }
  if (!window.isSecureContext || !window.crypto?.subtle) {
    out.push({ code: 'INSECURE_CONTEXT', title: '보안 연결이 아니에요', body: '전자서명과 파일 확인 기능은 https 주소에서만 동작해요. https://studio.audeniq.com 으로 접속해 주세요.' });
  }
  return out;
}

// ---------------------------------------------------------------------------
// 창
// ---------------------------------------------------------------------------
function DialogBody({ kind, title, body, meta, actions }: {
  kind: ErrorKind; title: string; body: ReactNode; meta?: string[];
  actions: { label: string; onClick: () => void; primary?: boolean; busyLabel?: string }[];
}) {
  const close = useModalClose();
  // 누른 버튼은 바로 '…하는 중'으로 — 새로고침 전 확인하는 동안 화면이 그대로라 눌리지 않은 것처럼 보였다
  const [busy, setBusy] = useState('');
  return (
    <div className={`aq-sysdialog is-${kind}`}>
      <span className="aq-sysdialog-icon"><ErrorIcon kind={kind} /></span>
      <h2>{title}</h2>
      <div className="aq-sysdialog-body">{body}</div>
      {meta && <p className="aq-sysdialog-meta">{meta.join(' · ')}</p>}
      <div className="aq-sysdialog-actions">
        {actions.map(a => (
          <button key={a.label} type="button" className={`ui-btn${a.primary ? ' is-primary' : ' is-soft'}${busy === a.label ? ' is-busy' : ''}`}
            disabled={!!busy} aria-busy={busy === a.label || undefined}
            onClick={() => { if (a.busyLabel) setBusy(a.label); a.onClick(); if (!a.primary) close(); }}>
            {busy === a.label && a.busyLabel ? a.busyLabel : a.label}
          </button>
        ))}
      </div>
    </div>
  );
}

export function SystemStatus({ children }: { children: ReactNode }) {
  const toast = useToast();
  const loc = useLocation();
  const loginFrom = loginRedirect(loc.state);
  // Matches site_content::gate's admin bypass: keep recovery controls reachable.
  // Also keep them navigable during failures; individual requests surface errors.
  const adminPage = loc.pathname.startsWith('/admin') || loc.pathname.startsWith('/content-admin')
    || (loc.pathname === '/login' && !!loginFrom?.startsWith('/admin'));

  const [online, setOnline] = useState(() => navigator.onLine);
  const [active, setActive] = useState<MaintenanceWindow | null>(null);
  const [upcoming, setUpcoming] = useState<MaintenanceWindow | null>(null);
  const [serverSaysMaintenance, setServerSaysMaintenance] = useState(false);
  const [serverIssue, setServerIssue] = useState<{ issue: ServerIssue; meta: string[] } | null>(null);
  const [updateReady, setUpdateReady] = useState(false);
  const [deviceIssues, setDeviceIssues] = useState<DeviceIssue[]>([]);
  const [dismissed, setDismissed] = useState(() => safeGet('local', DISMISS_KEY) ?? '');
  const wasMaintenance = useRef(false);
  const updateDismissedFor = useRef<string | null>(null);

  // --- 서버 점검 상태 ---
  // 탭 전환·온라인 복귀가 겹쳐도 상태 확인은 5초에 한 번만
  const lastStatusAt = useRef(0);
  const statusPending = useRef(false);
  const issueRevision = useRef(0);
  const refreshStatus = useCallback(async () => {
    if (statusPending.current || Date.now() - lastStatusAt.current < 5_000) return;
    lastStatusAt.current = Date.now();
    const revision = issueRevision.current;
    statusPending.current = true;
    const st = await fetchStatus().finally(() => { statusPending.current = false; });
    // A probe started before a newer failure cannot establish recovery.
    if (!st || revision !== issueRevision.current) return;
    setActive(st.maintenance.active);
    setUpcoming(st.maintenance.upcoming);
    if (!st.maintenance.active) setServerSaysMaintenance(false);
    // 상태 확인이 다시 되면 서버는 살아있다는 뜻이다. 전체 화면 서버 장애를
    // 내린다. 다음 실패는 시간 제한 없이 다시 표시한다.
    setServerIssue(null);
  }, []);

  const maintenance = !!active || serverSaysMaintenance;
  const serverDown = !!serverIssue;
  useEffect(() => {
    void refreshStatus();
    const t = window.setInterval(() => {
      if (document.visibilityState === 'visible') void refreshStatus();
    }, maintenance || serverDown ? STATUS_DURING_SERVER_DOWN_MS : STATUS_EVERY_MS);
    // 다른 탭에 있다가 돌아오면 바로 확인
    const onVisible = () => { if (document.visibilityState === 'visible') void refreshStatus(); };
    document.addEventListener('visibilitychange', onVisible);
    return () => { window.clearInterval(t); document.removeEventListener('visibilitychange', onVisible); };
  }, [refreshStatus, maintenance, serverDown]);

  // 점검이 끝나면 알린다 (앱은 그대로 남아 있어 이어서 쓸 수 있다)
  // 점검과 서버 장애는 같은 전체 화면 레이어를 쓰므로, 본문 스크롤 잠금도
  // 둘 다에 적용한다. 서버 장애만 있을 때는 이 잠금이 빠져 있었다.
  useEffect(() => {
    if (wasMaintenance.current && !maintenance) toast('점검이 끝났어요. 이어서 이용할 수 있어요.', 'success');
    wasMaintenance.current = maintenance;
    if ((!maintenance && !serverDown) || adminPage) return;
    const prev = document.body.style.overflow;
    document.body.style.overflow = 'hidden';
    return () => { document.body.style.overflow = prev; };
  }, [maintenance, serverDown, adminPage, toast]);

  // --- 온라인/오프라인 ---
  useEffect(() => {
    const on = () => { setOnline(true); toast('인터넷에 다시 연결됐어요.', 'success'); void refreshStatus(); };
    const off = () => setOnline(false);
    window.addEventListener('online', on);
    window.addEventListener('offline', off);
    return () => { window.removeEventListener('online', on); window.removeEventListener('offline', off); };
  }, [toast, refreshStatus]);

  // --- 서버 장애·점검 응답 ---
  useEffect(() => {
    const onIssue = (e: Event) => {
      const issue = (e as CustomEvent<ServerIssue>).detail;
      if (!issue) return;
      // Every new failure invalidates older successful probes. The existing
      // visible dialog is deduplicated by setServerIssue, never by a time window.
      if (issue.kind === 'maintenance') {
        issueRevision.current++;
        const w = issue.window as MaintenanceWindow | undefined;
        if (w && typeof w === 'object' && typeof w.starts_at === 'string') setActive(w);
        setServerSaysMaintenance(true);
        return;
      }
      if (!navigator.onLine) return; // 오프라인은 배너로 안내
      issueRevision.current++;
      setServerIssue(cur => cur ?? { issue, meta: incidentMeta(issue.status ? `${issue.code} (${issue.status})` : issue.code) });
    };
    window.addEventListener(SERVER_ISSUE_EVENT, onIssue);
    return () => window.removeEventListener(SERVER_ISSUE_EVENT, onIssue);
  }, [refreshStatus]);

  // --- 새 버전 ---
  useEffect(() => {
    if (import.meta.env.DEV) return;
    const mine = currentEntry();
    if (!mine) return;
    let busy = false;
    // 탭을 자주 오가도 index를 매번 받지 않게 — 마지막 확인 뒤 1분이 지났을 때만
    let lastAt = Date.now();
    const check = async () => {
      if (busy || document.visibilityState !== 'visible' || Date.now() - lastAt < VERSION_MIN_GAP_MS) return;
      busy = true;
      lastAt = Date.now();
      const latest = await latestEntry();
      busy = false;
      // 이 버전으로 가려고 이미 새로고침했는데도 그대로면(엣지·브라우저 캐시가 옛 화면을 준 경우) 다시 묻지 않는다 —
      // 새로고침해도 창이 계속 떠 화면을 쓸 수 없었다
      if (latest && latest !== mine && latest !== updateDismissedFor.current && safeGet('session', RELOADED_FOR_KEY) !== latest) setUpdateReady(true);
    };
    const t = window.setInterval(() => void check(), VERSION_EVERY_MS);
    const onVisible = () => void check();
    document.addEventListener('visibilitychange', onVisible);
    return () => { window.clearInterval(t); document.removeEventListener('visibilitychange', onVisible); };
  }, []);

  // --- 기기 점검 (탭마다 한 번) ---
  useEffect(() => {
    if (safeGet('session', DEVICE_KEY)) return;
    const found = detectDeviceIssues();
    if (found.length) setDeviceIssues(found);
    else safeSet('session', DEVICE_KEY, '1');
  }, []);

  // 점검·서버 장애 중이면 앱 위에 전체 화면을 덮는다 (관리 화면은 그대로 열어 둔다).
  // 앱을 내리지 않으므로 작성 중이던 입력은 복구 뒤 그대로 이어서 쓸 수 있다.
  // 닫기 버튼을 두지 않는다: 닫으면 뒤 로그인 화면이 보이는 것이 2026-10-28 스크린샷의 문제였다.
  const blocking = (maintenance || serverDown) && !adminPage;
  const emergency = active?.kind === 'emergency';
  const overlay = blocking && createPortal(
    <div className="aq-maint-layer" role="dialog" aria-modal="true" aria-label={maintenance ? '서버 점검' : '서버 장애'}>
      {maintenance ? (
        <ErrorScreen
          kind="maintenance" fullPage
          eyebrow={emergency ? '긴급 점검' : '서버 점검'}
          title={emergency ? '긴급 점검 중이에요' : '지금은 서버 점검 중이에요'}
          description={(
            <>
              {active && <p className="aq-errscreen-window">{windowLabel(active)}</p>}
              <p>
                {active?.body || (emergency
                  ? '서비스를 안정적으로 되돌리고 있어요. 끝나는 대로 자동으로 다시 열려요.'
                  : '점검이 끝나면 자동으로 다시 열려요.')}
              </p>
              <p>이 창을 닫지 않으면 작성하던 내용은 그대로 남아 있어요.</p>
            </>
          )}
          actions={[{ label: '상태 다시 확인', onClick: () => void refreshStatus(), primary: true }]}
          meta={[active ? active.title : '서버 점검', '30초마다 자동으로 확인해요']}
        />
      ) : serverIssue ? (
        <ErrorScreen
          kind="server" fullPage
          eyebrow="서버 장애"
          title="서버에 연결하지 못했어요"
          description={(
            <>
              <p>잠시 서버가 응답하지 않아요. 서버가 복구되면 이 화면은 자동으로 사라져요. 새로고침하면 저장하지 않은 입력은 사라질 수 있어요.</p>
              <p>계속되면 아래 오류 코드와 함께 문의해 주세요.</p>
            </>
          )}
          actions={[{ label: '상태 다시 확인', onClick: () => void refreshStatus(), primary: true }]}
          meta={[...serverIssue.meta, '30초마다 자동으로 확인해요']}
        />
      ) : null}
    </div>,
    document.body,
  );

  const upcomingKey = upcoming ? `${upcoming.id}@${upcoming.updated_at}` : '';
  const showUpcoming = !!upcoming && dismissed !== upcomingKey && !adminPage;

  return (
    <>
      {(!online || showUpcoming) && (
        <div className="aq-sysbar-stack" role="status" aria-live="polite">
          {!online && (
            <div className="aq-sysbar is-offline">
              <ErrorIcon kind="offline" />
              <span><strong>인터넷 연결이 끊겼어요.</strong> 연결되면 이어서 저장할 수 있어요.</span>
            </div>
          )}
          {showUpcoming && upcoming && (
            <div className="aq-sysbar is-upcoming">
              <ErrorIcon kind="maintenance" />
              <span><strong>{upcoming.title}</strong> {windowLabel(upcoming)} 동안 스튜디오를 이용할 수 없어요.</span>
              <button
                type="button" className="aq-sysbar-close" aria-label="점검 예고 닫기"
                onClick={() => { safeSet('local', DISMISS_KEY, upcomingKey); setDismissed(upcomingKey); }}
              ><Glyph name="close" size={14} /></button>
            </div>
          )}
        </div>
      )}
      <div className="aq-app-shell" inert={blocking || undefined} aria-hidden={blocking || undefined}>{children}</div>
      {overlay}

      {/* Version and device state are already independent; priority prevents
          stacked modals, and dismissing the version prompt reveals device issues. */}
      {!blocking && !serverIssue && updateReady && (
        <Modal title="새 버전이 준비됐어요" onClose={() => { updateDismissedFor.current = currentEntry(); setUpdateReady(false); }} modalClass="aq-sysdialog-mode">
          <DialogBody
            kind="update"
            title="새 버전이 준비됐어요"
            body={<p>스튜디오가 업데이트됐어요. 새로고침하면 최신 기능과 수정 사항이 적용돼요. 작성 중인 내용이 있으면 저장한 뒤 새로고침해 주세요.</p>}
            actions={[
              { label: '나중에', onClick: () => { void latestEntry().then(v => { updateDismissedFor.current = v; }); setUpdateReady(false); } },
              { label: '지금 새로고침', busyLabel: '새로고침하는 중', primary: true, onClick: () => { void latestEntry(3000).then(v => { if (v) safeSet('session', RELOADED_FOR_KEY, v); reloadFresh(); }); } },
            ]}
          />
        </Modal>
      )}

      {!blocking && !serverIssue && !updateReady && deviceIssues.length > 0 && (
        <Modal title="기기 확인이 필요해요" onClose={() => { safeSet('session', DEVICE_KEY, '1'); setDeviceIssues([]); }} modalClass="aq-sysdialog-mode">
          <DialogBody
            kind="device"
            title={deviceIssues.length > 1 ? '이 기기에서 확인이 필요해요' : deviceIssues[0].title}
            body={deviceIssues.length > 1 ? (
              <ul className="aq-sysdialog-list">
                {deviceIssues.map(d => <li key={d.code}><strong>{d.title}</strong><span>{d.body}</span></li>)}
              </ul>
            ) : <p>{deviceIssues[0].body}</p>}
            meta={deviceIssues.map(d => `오류 코드 ${d.code}`)}
            actions={[{ label: '확인', onClick: () => { safeSet('session', DEVICE_KEY, '1'); setDeviceIssues([]); } }]}
          />
        </Modal>
      )}
    </>
  );
}
