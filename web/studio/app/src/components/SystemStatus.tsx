// 서비스·기기 상태 알림
// - 서버 점검: /api/status(D1)에 진행 중인 점검이 있거나 서버가 MAINTENANCE로 답하면 점검 화면, 72시간 안의 점검은 상단 예고
// - 서버 장애: API가 502/503/504·연결 실패면 오류 창 (1분에 한 번까지)
// - 새 버전: 배포로 index의 앱 번들이 바뀌면 새로고침 안내 창
// - 기기 문제: 오프라인 배너, 쿠키·저장소 차단, 보안 연결이 아닌 경우 안내 창
import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import { Modal, useModalClose } from './Modal';
import { ErrorIcon, ErrorScreen, incidentMeta, type ErrorKind } from './ErrorScreen';
import { useToast } from './Toast';
import { useLocation } from '../lib/router';
import { SERVER_ISSUE_EVENT, type ServerIssue } from '../lib/systemEvents';
import { fetchStatus, type MaintenanceWindow } from '../api/content';

const STATUS_EVERY_MS = 5 * 60_000;
const STATUS_DURING_MAINTENANCE_MS = 60_000;
const VERSION_EVERY_MS = 5 * 60_000;
const SERVER_DIALOG_COOLDOWN_MS = 60_000;

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

/** 점검 기간 문구 — 같은 날이면 끝 시각만 */
export function windowLabel(w: Pick<MaintenanceWindow, 'starts_at' | 'ends_at'>): string {
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
async function latestEntry(): Promise<string | null> {
  try {
    const res = await fetch(import.meta.env.BASE_URL, { cache: 'no-store', credentials: 'same-origin', headers: { Accept: 'text/html' } });
    if (!res.ok) return null;
    const m = ENTRY_RE.exec(await res.text());
    return m ? new URL(m[1], window.location.href).pathname : null;
  } catch {
    return null;
  }
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
  actions: { label: string; onClick: () => void; primary?: boolean }[];
}) {
  const close = useModalClose();
  return (
    <div className={`aq-sysdialog is-${kind}`}>
      <span className="aq-sysdialog-icon"><ErrorIcon kind={kind} /></span>
      <h2>{title}</h2>
      <div className="aq-sysdialog-body">{body}</div>
      {meta && <p className="aq-sysdialog-meta">{meta.join(' · ')}</p>}
      <div className="aq-sysdialog-actions">
        {actions.map(a => (
          <button key={a.label} type="button" className={`button${a.primary ? '' : ' secondary'}`} onClick={() => { a.onClick(); if (!a.primary) close(); }}>
            {a.label}
          </button>
        ))}
      </div>
    </div>
  );
}

export function SystemStatus({ children }: { children: ReactNode }) {
  const toast = useToast();
  const loc = useLocation();
  const adminPage = loc.pathname.startsWith('/content-admin');

  const [online, setOnline] = useState(() => navigator.onLine);
  const [active, setActive] = useState<MaintenanceWindow | null>(null);
  const [upcoming, setUpcoming] = useState<MaintenanceWindow | null>(null);
  const [serverSaysMaintenance, setServerSaysMaintenance] = useState(false);
  const [serverIssue, setServerIssue] = useState<{ issue: ServerIssue; meta: string[] } | null>(null);
  const [updateReady, setUpdateReady] = useState(false);
  const [deviceIssues, setDeviceIssues] = useState<DeviceIssue[]>([]);
  const [dismissed, setDismissed] = useState(() => safeGet('local', DISMISS_KEY) ?? '');
  const lastServerDialog = useRef(0);
  const updateDismissedFor = useRef<string | null>(null);

  // --- 서버 점검 상태 ---
  const refreshStatus = useCallback(async () => {
    const st = await fetchStatus();
    if (!st) return;
    setActive(st.maintenance.active);
    setUpcoming(st.maintenance.upcoming);
    if (!st.maintenance.active) setServerSaysMaintenance(false);
  }, []);

  const maintenance = !!active || serverSaysMaintenance;
  useEffect(() => {
    void refreshStatus();
    const t = window.setInterval(() => void refreshStatus(), maintenance ? STATUS_DURING_MAINTENANCE_MS : STATUS_EVERY_MS);
    return () => window.clearInterval(t);
  }, [refreshStatus, maintenance]);

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
      if (issue.kind === 'maintenance') { setServerSaysMaintenance(true); void refreshStatus(); return; }
      if (!navigator.onLine) return; // 오프라인은 배너로 안내
      const now = Date.now();
      if (now - lastServerDialog.current < SERVER_DIALOG_COOLDOWN_MS) return;
      lastServerDialog.current = now;
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
    const check = async () => {
      if (busy || document.visibilityState !== 'visible') return;
      busy = true;
      const latest = await latestEntry();
      busy = false;
      if (latest && latest !== mine && latest !== updateDismissedFor.current) setUpdateReady(true);
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

  // 점검 중이면 스튜디오 대신 점검 화면 (관리 화면은 그대로 열어 둔다)
  if (maintenance && !adminPage) {
    return (
      <ErrorScreen
        kind="maintenance" fullPage
        eyebrow="서버 점검"
        title="지금은 서버 점검 중이에요"
        description={active ? (
          <>
            <p className="aq-errscreen-window">{windowLabel(active)}</p>
            <p>{active.body || '점검이 끝나면 자동으로 다시 열려요. 작성하던 내용은 그대로 저장돼 있어요.'}</p>
          </>
        ) : <p>점검이 끝나면 자동으로 다시 열려요. 작성하던 내용은 그대로 저장돼 있어요.</p>}
        actions={[{ label: '상태 다시 확인', onClick: () => void refreshStatus(), primary: true }]}
        meta={[active ? active.title : '서버 점검', '1분마다 자동으로 확인해요']}
      />
    );
  }

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
              >×</button>
            </div>
          )}
        </div>
      )}
      {children}

      {serverIssue && (
        <Modal title="서버에 연결하지 못했어요" onClose={() => setServerIssue(null)} modalClass="aq-sysdialog-mode">
          <DialogBody
            kind="server"
            title="서버에 연결하지 못했어요"
            body={<p>잠시 서버가 응답하지 않아요. 입력한 내용은 이 기기에 남아 있으니, 잠시 뒤 다시 시도해 주세요. 계속되면 아래 오류 코드와 함께 문의해 주세요.</p>}
            meta={serverIssue.meta}
            actions={[
              { label: '닫기', onClick: () => setServerIssue(null) },
              { label: '새로고침', onClick: () => window.location.reload(), primary: true },
            ]}
          />
        </Modal>
      )}

      {!serverIssue && updateReady && (
        <Modal title="새 버전이 준비됐어요" onClose={() => { updateDismissedFor.current = currentEntry(); setUpdateReady(false); }} modalClass="aq-sysdialog-mode">
          <DialogBody
            kind="update"
            title="새 버전이 준비됐어요"
            body={<p>스튜디오가 업데이트됐어요. 새로고침하면 최신 기능과 수정 사항이 적용돼요. 작성 중인 내용이 있으면 저장한 뒤 새로고침해 주세요.</p>}
            actions={[
              { label: '나중에', onClick: () => { void latestEntry().then(v => { updateDismissedFor.current = v; }); setUpdateReady(false); } },
              { label: '지금 새로고침', onClick: () => window.location.reload(), primary: true },
            ]}
          />
        </Modal>
      )}

      {!serverIssue && !updateReady && deviceIssues.length > 0 && (
        <Modal title="기기 확인이 필요해요" onClose={() => { safeSet('session', DEVICE_KEY, '1'); setDeviceIssues([]); }} modalClass="aq-sysdialog-mode">
          <DialogBody
            kind="device"
            title={deviceIssues.length > 1 ? '이 기기에서 확인이 필요해요' : deviceIssues[0].title}
            body={deviceIssues.length > 1 ? (
              <ul className="aq-sysdialog-list">
                {deviceIssues.map(d => <li key={d.code}><strong>{d.title}</strong><span>{d.body}</span></li>)}
              </ul>
            ) : <p>{deviceIssues[0].body}</p>}
            meta={deviceIssues.map(d => d.code)}
            actions={[{ label: '확인', onClick: () => { safeSet('session', DEVICE_KEY, '1'); setDeviceIssues([]); } }]}
          />
        </Modal>
      )}
    </>
  );
}
