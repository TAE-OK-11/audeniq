import { useEffect, useState } from 'react';
import { useNavigate } from '../lib/router';
import { Modal } from '../components/Modal';
import { useToast } from '../components/Toast';
import { localStamp } from '../lib/format';
import { parseStamp, relativeTime } from '../lib/date';
import { markAllNoticesRead, markNoticeRead, useNotices, useUnreadCount, type Notice } from '../store/support';
import { Glyph } from '../components/Glyph';
import { alertsBlocked, alertsEnabled, alertsSupported, disableAlerts, enableAlerts } from '../lib/alerts';
import { MOCK } from '../lib/mode';
import * as portal from '../api/portal';
import { errorMessage } from '../api/errors';

/** 브라우저 알림 켜기 — 다른 탭·창에 있어도 답변·검토 결과를 바로 알려 준다 */
function BrowserAlerts() {
  const toast = useToast();
  const [on, setOn] = useState(alertsEnabled);
  if (!alertsSupported() || MOCK) return null;
  const blocked = alertsBlocked();
  const toggle = async (next: boolean) => {
    if (!next) { disableAlerts(); setOn(false); toast('브라우저 알림을 껐어요.', 'info'); return; }
    const ok = await enableAlerts();
    setOn(ok);
    toast(ok ? '브라우저 알림을 켰어요. 다른 탭에 있어도 알려 드릴게요.' : '브라우저에서 알림 권한을 허용해 주세요.', ok ? 'success' : 'error');
  };
  return (
    <div className="aq-alert-pref">
      <span className="aq-alert-pref-icon" aria-hidden="true"><Glyph name="bell" size={18} /></span>
      <span className="min-0">
        <strong>브라우저 알림</strong>
        <span>{blocked ? '브라우저 설정에서 이 사이트의 알림이 차단돼 있어요. 주소창의 자물쇠 아이콘에서 허용해 주세요.' : '문의 답변·검토 결과·지급 소식을 다른 탭에 있어도 바로 알려 드려요.'}</span>
      </span>
      <label className="aq-switch">
        <input type="checkbox" role="switch" aria-label="브라우저 알림" checked={on} disabled={blocked} onChange={e => void toggle(e.target.checked)} />
        <span aria-hidden="true" />
      </label>
    </div>
  );
}

/** 알림 메일 — 같은 알림을 로그인 이메일로도 받는다 (서버 설정, 기본 켜짐) */
function EmailAlerts() {
  const toast = useToast();
  const [on, setOn] = useState<boolean | null>(null);
  const [saving, setSaving] = useState(false);
  useEffect(() => {
    if (MOCK) return;
    let alive = true;
    portal.fetchNotificationSettings().then(v => { if (alive) setOn(v.emailEnabled); }).catch(() => { if (alive) setOn(null); });
    return () => { alive = false; };
  }, []);
  if (MOCK || on === null) return null;
  const toggle = async (next: boolean) => {
    setSaving(true);
    try {
      const v = await portal.saveNotificationSettings(next);
      setOn(v.emailEnabled);
      toast(v.emailEnabled ? '알림 메일을 켰어요. 로그인 이메일로 함께 보내 드릴게요.' : '알림 메일을 껐어요.', v.emailEnabled ? 'success' : 'info');
    } catch (e) {
      toast(errorMessage(e), 'error');
    } finally {
      setSaving(false);
    }
  };
  return (
    <div className="aq-alert-pref">
      <span className="aq-alert-pref-icon" aria-hidden="true"><Glyph name="mail" size={18} /></span>
      <span className="min-0">
        <strong>알림 메일</strong>
        <span>검토 결과·서류 요청·정산·문의 답변 알림을 로그인 이메일로도 보내 드려요.</span>
      </span>
      <label className="aq-switch">
        <input type="checkbox" role="switch" aria-label="알림 메일" checked={on} disabled={saving} onChange={e => void toggle(e.target.checked)} />
        <span aria-hidden="true" />
      </label>
    </div>
  );
}

/** 날짜 묶음 — 오늘 · 어제 · 지난 7일, 그 전은 달마다 ('9월', 해가 다르면 '2025년 12월') */
function noticeGroup(time: string, now: Date): string {
  const d = parseStamp(time);
  if (!d) return '이전';
  const day = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const days = Math.round((day(now) - day(d)) / 86_400_000);
  if (days <= 0) return '오늘';
  if (days === 1) return '어제';
  if (days < 7) return '지난 7일';
  return d.getFullYear() === now.getFullYear() ? `${d.getMonth() + 1}월` : `${d.getFullYear()}년 ${d.getMonth() + 1}월`;
}

/** 줄 오른쪽 시각 — 묶음과 맞춘다: 오늘은 '12분 전', 어제는 '오후 4:50', 그 전은 '10월 26일'
 * ('어제' 묶음에 '12시간 전', '지난 7일'에 '1일 전'이 섞이면 묶음과 시각이 서로 다른 말을 했다) */
function noticeTime(time: string, group: string, now: Date): string {
  const d = parseStamp(time);
  if (!d) return time;
  if (group === '오늘') return relativeTime(time, now);
  if (group === '어제') return d.toLocaleTimeString('ko-KR', { hour: 'numeric', minute: '2-digit' });
  return `${d.getMonth() + 1}월 ${d.getDate()}일`;
}

/** 처음엔 이만큼만 — 알림이 수십 개 쌓이면 화면이 끝없이 길어졌다 */
const PAGE = 20;

function noticeSymbol(kind: string) {
  return <Glyph size={17} name={kind === '지급' ? 'won' : kind === '발매' ? 'music' : kind === '서류' ? 'doc' : kind === '문의' ? 'mail' : kind === '계정' ? 'user' : 'bell'} />;
}

export function Notifications() {
  const toast = useToast();
  const nav = useNavigate();
  const [filter, setFilter] = useState<'all' | 'unread'>('all');
  const notices = useNotices();
  const unread = useUnreadCount();
  const [openNotice, setOpenNotice] = useState<Notice | null>(null);
  const [limit, setLimit] = useState(PAGE);

  const shown = (filter === 'all' ? notices : notices.filter(n => !n.read))
    .slice().sort((a, b) => b.time.localeCompare(a.time));
  const now = new Date();
  const groups: { label: string; items: Notice[] }[] = [];
  for (const n of shown.slice(0, limit)) {
    const label = noticeGroup(n.time, now);
    const last = groups[groups.length - 1];
    if (last?.label === label) last.items.push(n); else groups.push({ label, items: [n] });
  }
  const more = shown.length - limit;

  const readAll = () => {
    if (!unread) { toast('읽지 않은 알림이 없어요.', 'info'); return; }
    markAllNoticesRead();
    toast('모든 알림을 읽음으로 표시했어요.');
  };

  const openNoticeDetail = (n: Notice) => {
    markNoticeRead(n.id);
    setOpenNotice(n);
  };

  return (
    <div id="view-notifications" className="view">
      <div className="ui-page-head">
        <div>
          <p className="eyebrow">계정·지원</p>
          <h1>알림</h1>
          <p>발매·정산·서류에 생긴 변화를 알려 드려요.</p>
        </div>
      </div>

      <BrowserAlerts />
      <EmailAlerts />

      <div className="ui-chips is-wrap" id="notificationTools">
        <button
          type="button"
          className={`ui-chip${filter === 'all' ? ' is-on' : ''}`}
          aria-pressed={filter === 'all'}
          onClick={() => { setFilter('all'); setLimit(PAGE); }}
        >전체 알림</button>
        <button
          type="button"
          className={`ui-chip${filter === 'unread' ? ' is-on' : ''}`}
          aria-pressed={filter === 'unread'}
          onClick={() => { setFilter('unread'); setLimit(PAGE); }}
        >읽지 않음 <span id="unreadCount">{unread}</span></button>
        <button type="button" className="ui-btn is-text" id="readAllNotices" onClick={readAll} disabled={!unread}><Glyph name="check" size={14} />모두 읽음</button>
      </div>

      {shown.length ? (
        // 날짜로 묶은 촘촘한 목록 — 알림 하나가 세 줄(분류·시각 / 제목 / 설명)씩 차지해 수십 개면 화면이 끝없이 길었다.
        // 분류는 아이콘으로, 시각은 오른쪽 위 작은 글자로, 설명은 한 줄. 안 읽은 알림은 아이콘의 파란 점과 굵은 제목
        <div className="aq-notice-feed" key={filter}>
          {groups.map(g => (
            <section key={g.label} className="aq-notice-group" aria-label={g.label}>
              <h2 className="aq-notice-day">{g.label}</h2>
              <ul className="aq-notice-rows">
                {g.items.map(n => (
                  <li key={n.id}>
                    <button type="button" className={`aq-notice-row${n.read ? ' is-read' : ''}`} onClick={() => openNoticeDetail(n)}>
                      <span className="aq-notice-symbol" aria-hidden="true">{noticeSymbol(n.kind)}{!n.read && <i className="aq-unread" />}</span>
                      <span className="aq-notice-copy">
                        <strong>{n.title}</strong>
                        {n.detail && <span className="aq-notice-detail">{n.detail}</span>}
                      </span>
                      <time className="aq-notice-time" dateTime={n.time}>{noticeTime(n.time, g.label, now)}</time>
                      {!n.read && <span className="sr-only">읽지 않음</span>}
                    </button>
                  </li>
                ))}
              </ul>
            </section>
          ))}
          {more > 0 && (
            <button type="button" className="ui-btn is-soft is-block aq-notice-more" onClick={() => setLimit(l => l + PAGE)}>
              알림 더 보기 <span>{more}개</span>
            </button>
          )}
        </div>
      ) : (
        <div className="ui-empty">
          <h2>{filter === 'unread' ? '읽지 않은 알림이 없어요.' : '새 알림이 없어요.'}</h2>
          <p>{filter === 'unread' ? '새 알림이 오면 여기에 먼저 보여요.' : '발매·정산·서류에 변화가 생기면 여기에 알려 드려요.'}</p>
          {filter === 'unread' && <button type="button" className="ui-btn is-soft" onClick={() => setFilter('all')}>전체 알림 보기</button>}
        </div>
      )}

      {openNotice && (
        <Modal title={openNotice.title} onClose={() => setOpenNotice(null)}>
          <div className="studio-notice-detail">
            <span className="eyebrow">{openNotice.kind || '알림'} · {localStamp(openNotice.time)}</span>
            <p>{openNotice.detail || '상세 내용이 없어요.'}</p>
            {openNotice.link ? (
              <div className="aq-sticky-foot">
                <button
                  type="button" className="ui-btn is-primary studio-submit-wide"
                  onClick={() => { const to = openNotice.link!; setOpenNotice(null); nav(to); }}
                >관련 화면으로 이동 <Glyph name="chevron-right" size={13} /></button>
              </div>
            ) : (
              <div className="doc-connection">관련 발매 또는 정산 내역은 각 관리 화면에서 확인할 수 있어요.</div>
            )}
          </div>
        </Modal>
      )}
    </div>
  );
}
