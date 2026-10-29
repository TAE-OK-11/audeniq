import { useEffect, useState } from 'react';
import { useNavigate } from '../lib/router';
import { Modal } from '../components/Modal';
import { useToast } from '../components/Toast';
import { localStamp } from '../lib/format';
import { jumpToTop } from '../lib/motion';
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

/** 한 쪽에 보여 줄 알림 수 — 끝없이 이어지지 않게 쪽 번호로 나눈다 */
const PAGE = 15;

/** 쪽 번호 — 지금 쪽 앞뒤 둘씩, 처음·끝 쪽은 늘 보이고 사이는 '…' */
function pageList(page: number, pages: number): (number | '…')[] {
  const out: (number | '…')[] = [];
  for (let p = 1; p <= pages; p++) {
    if (p === 1 || p === pages || Math.abs(p - page) <= 2) out.push(p);
    else if (out[out.length - 1] !== '…') out.push('…');
  }
  return out;
}

function Pager({ page, pages, onPage }: { page: number; pages: number; onPage: (p: number) => void }) {
  if (pages <= 1) return null;
  return (
    <nav className="aq-pager" aria-label="알림 쪽">
      <button type="button" className="aq-pager-step" onClick={() => onPage(page - 1)} disabled={page <= 1} aria-label="이전 쪽"><Glyph name="chevron-left" size={16} /></button>
      {pageList(page, pages).map((p, i) => p === '…'
        ? <span key={`gap${i}`} className="aq-pager-gap" aria-hidden="true">…</span>
        : <button key={p} type="button" className={`aq-pager-num${p === page ? ' is-on' : ''}`} aria-current={p === page ? 'page' : undefined} onClick={() => onPage(p)}>{p}</button>)}
      <button type="button" className="aq-pager-step" onClick={() => onPage(page + 1)} disabled={page >= pages} aria-label="다음 쪽"><Glyph name="chevron-right" size={16} /></button>
    </nav>
  );
}

function noticeSymbol(kind: string) {
  return <Glyph size={17} name={kind === '지급' ? 'won' : kind === '발매' ? 'music' : kind === '서류' ? 'doc' : kind === '문의' ? 'mail' : kind === '계정' ? 'user' : 'bell'} />;
}

/** 같은 알림 묶음 — 바로 이어서 온 같은 제목의 알림(같은 발매의 보완 요청 등)은 카드 하나에 '외 N건'으로.
 * 같은 줄이 여러 번 쌓이면 무엇이 새로운지 가려졌다 (알림 센터 설계 글들이 꼽는 가장 효과 큰 정리) */
interface NoticeGroup { head: Notice; all: Notice[] }
function bundle(list: Notice[]): NoticeGroup[] {
  const out: NoticeGroup[] = [];
  for (const n of list) {
    const last = out[out.length - 1];
    if (last && last.head.title === n.title && last.head.read === n.read) last.all.push(n);
    else out.push({ head: n, all: [n] });
  }
  return out;
}

/** 지난 알림의 시간 묶음 — 이번 주(7일 안) · 이전 */
const WEEK = 7 * 86_400_000;
const isThisWeek = (n: Notice, now: number) => { const d = parseStamp(n.time); return !!d && now - d.getTime() < WEEK; };

/** 알림 카드 하나 — 누르면 내용 창(관련 화면이 있으면 창 아래 버튼으로 이동).
 * 아이콘(분류) · 제목 · 시각 한 줄 · 꺾쇠. 안 읽음은 파란 점과 굵은 제목 하나로만 */
function NoticeCard({ g, onOpen }: { g: NoticeGroup; onOpen: (g: NoticeGroup) => void }) {
  const n = g.head;
  const more = g.all.length - 1;
  return (
    <button type="button" className={`aq-notice-card${n.read ? ' is-read' : ''}`} onClick={() => onOpen(g)} aria-label={`${n.kind || '안내'} 알림${n.read ? '' : ', 읽지 않음'}: ${n.title}${more ? `, 같은 알림 ${more}건 더` : ''}`}>
      <span className="aq-notice-symbol" aria-hidden="true">{noticeSymbol(n.kind)}{!n.read && <i className="aq-unread" />}</span>
      <span className="aq-notice-copy">
        <strong>{n.title}</strong>
        {/* 목록에는 제목과 시각만 — 설명은 한 줄에서 '…'로 잘려 읽을 수 없었다. 전체 내용은 눌러서 여는 창에서 */}
        <span className="aq-notice-detail"><time dateTime={n.time}>{relativeTime(n.time) || n.time}</time>{more > 0 && <> · 같은 알림 {more}건 더</>}</span>
      </span>
      <Glyph name="chevron-right" size={18} className="aq-notice-chev" />
    </button>
  );
}

export function Notifications() {
  const toast = useToast();
  const nav = useNavigate();
  const notices = useNotices();
  const unread = useUnreadCount();
  const [openNotice, setOpenNotice] = useState<Notice | null>(null);
  const [page, setPage] = useState(1);

  // 새 알림(안 읽음)을 위에, 그 아래 지난 알림(이번 주 · 이전) — 각각 최신순, 이어진 같은 알림은 한 카드로.
  // 쪽 번호는 카드 수로 센다
  const byTime = (a: Notice, b: Notice) => b.time.localeCompare(a.time);
  const now = Date.now();
  const freshAll = bundle(notices.filter(n => !n.read).sort(byTime));
  const pastAll = bundle(notices.filter(n => n.read).sort(byTime));
  const cards: { group: NoticeGroup; section: 'fresh' | 'week' | 'older' }[] = [
    ...freshAll.map(group => ({ group, section: 'fresh' as const })),
    ...pastAll.map(group => ({ group, section: isThisWeek(group.head, now) ? 'week' as const : 'older' as const })),
  ];
  const pages = Math.max(1, Math.ceil(cards.length / PAGE));
  const current = Math.min(page, pages);
  const pageCards = cards.slice((current - 1) * PAGE, current * PAGE);
  const sections = [
    { key: 'fresh', title: '새 알림' },
    { key: 'week', title: '이번 주' },
    { key: 'older', title: '이전 알림' },
  ].map(s => ({ ...s, items: pageCards.filter(c => c.section === s.key).map(c => c.group) })).filter(s => s.items.length);
  // 쪽을 바꾸면 목록 맨 위로 — 아래 쪽 번호를 눌렀는데 화면이 그대로 아래에 있으면 바뀐 줄 몰랐다.
  // 부드럽게 굴리지 않고 바로 옮긴다: 새 쪽 목록이 먼 거리를 미끄러져 지나가는 모습이 작위적이었다
  const goPage = (p: number) => {
    setPage(p);
    jumpToTop(document.getElementById('noticeList'));
  };

  const readAll = () => {
    markAllNoticesRead();
    toast('모든 알림을 읽음으로 표시했어요.');
  };

  // 누르면 내용 창 — 전체 설명을 읽고, 관련 화면이 있으면 창 아래 버튼으로 이동. 묶인 알림은 함께 읽음
  const openNoticeDetail = (g: NoticeGroup) => {
    g.all.forEach(n => markNoticeRead(n.id));
    setOpenNotice(g.head);
  };

  return (
    <div id="view-notifications" className="view">
      {/* 설명 줄 없이 짧게 — 머리가 길어 알림이 늦게 보였다 */}
      <div className="ui-page-head aq-notice-pagehead">
        <div>
          <p className="eyebrow">계정·지원</p>
          <h1>알림</h1>
        </div>
      </div>

      {cards.length ? (
        <div id="noticeList" className="aq-notice-sections">
          {/* 새 알림이 없으면 빈 칸 대신 한 줄로 알려 준다 */}
          {sections.map(sec => (
            <section key={sec.key} aria-labelledby={`notice-${sec.key}`}>
              <div className="aq-notice-head">
                <h2 id={`notice-${sec.key}`}>{sec.title}{sec.key === 'fresh' && <span>{unread}</span>}</h2>
                {sec.key === 'fresh' && <button type="button" className="ui-btn is-text is-sm" onClick={readAll}><Glyph name="check" size={14} />모두 읽음</button>}
              </div>
              <div className="aq-catalog-cards">{sec.items.map(g => <NoticeCard key={g.head.id} g={g} onOpen={openNoticeDetail} />)}</div>
            </section>
          ))}
          <Pager page={current} pages={pages} onPage={goPage} />
        </div>
      ) : (
        <div className="ui-empty">
          <h2>아직 알림이 없어요</h2>
          <p>발매·정산·서류에 변화가 생기면 여기에 알려 드려요.</p>
        </div>
      )}

      {/* 알림 받는 방법 — 목록을 밀어내지 않게 아래에 */}
      <section className="aq-notice-settings" aria-label="알림 받는 방법">
        <BrowserAlerts />
        <EmailAlerts />
      </section>

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
