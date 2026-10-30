// 출력물 식별 — 인쇄·PDF 저장을 누르면 서버가 문서 식별코드와 원본 검증코드를 발급하고,
// 모든 쪽의 위·아래에 찍은 뒤 인쇄한다(crates/core/src/print_stamp.rs).
// 브라우저가 넣던 머리말·꼬리말(시각·URL)은 @page 여백을 0으로 두어 빼고, 그 자리에 이 표식을 넣는다.
//
// 엔진마다 인쇄가 다르다:
// - Chrome·Edge·Firefox(PC·Android): position:fixed 요소를 쪽마다 반복한다 → 머리말·꼬리말을 모든 쪽에.
// - WebKit(Safari, 그리고 iPhone·iPad의 모든 브라우저): fixed 요소를 화면(뷰포트) 기준으로 한 번만 찍는다.
//   그대로 두면 꼬리말이 첫 쪽 중간(휴대폰 화면 높이)에 본문과 겹쳐 찍힌다. 그래서 WebKit에서는
//   머리말을 문서 맨 앞, 꼬리말을 문서 맨 끝에 흐름대로 두고 쪽 여백은 @page로 준다.
// 출력자·원본 검증코드는 꼬리말에만 찍는다. 본문 위에 비스듬히 반복하던 워터마크는 서류를 가려 뺐다.
import { useLayoutEffect, useSyncExternalStore } from 'react';
import { createPortal } from 'react-dom';
import { req } from '../api/http';
import { MOCK } from './mode';

export type PrintKind = 'TERMS' | 'SETTLEMENT_TERMS' | 'PRIVACY' | 'APPLICATION' | 'DOCUMENT' | 'SIGNING' | 'STAFF_SHEET';

export interface PrintStamp {
  document_code: string;
  verification_code: string;
  printed_at: string;
  printed_by: string;
  title: string;
  verify_url: string;
  /** 발급하지 못했을 때(오프라인·체험 모드): 원본으로 확인되지 않는 출력물임을 그대로 찍는다. */
  unverified?: string;
}

let current: PrintStamp | null = null;
const listeners = new Set<() => void>();
const set = (s: PrintStamp | null) => { current = s; listeners.forEach(l => l()); };
export function usePrintStamp(): PrintStamp | null {
  return useSyncExternalStore(cb => { listeners.add(cb); return () => listeners.delete(cb); }, () => current, () => null);
}

/** 이 브라우저의 기기 id (출력 기록에 남는다). 저장소를 못 쓰면 매번 새로 만든다. */
function deviceId(): string {
  const key = 'aq.device';
  try {
    const v = localStorage.getItem(key);
    if (v && /^[0-9a-f-]{36}$/.test(v)) return v;
    const id = crypto.randomUUID();
    localStorage.setItem(key, id);
    return id;
  } catch {
    return crypto.randomUUID();
  }
}

/** 화면에 보이는 문서 본문(공백 정리)의 SHA-256 */
async function contentHash(root: Element | null): Promise<string | undefined> {
  const text = (root as HTMLElement | null)?.innerText?.replace(/\s+/g, ' ').trim();
  if (!text || !crypto.subtle) return undefined;
  const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(text));
  return Array.from(new Uint8Array(digest), b => b.toString(16).padStart(2, '0')).join('');
}

const nextFrame = () => new Promise<void>(r => requestAnimationFrame(() => requestAnimationFrame(() => r())));

/**
 * 코드를 받아 찍고 인쇄한다. root: 해시를 낼 문서 영역(없으면 #main).
 * staff: 관리자 콘솔의 심사 기록 (스태프 전용 경로).
 */
export async function printWithStamp(doc: { kind: PrintKind; ref: string; title?: string; root?: Element | null; staff?: boolean }): Promise<void> {
  const root = doc.root ?? document.getElementById('main') ?? document.body;
  try {
    if (MOCK) throw new Error('체험 모드');
    const stamp = await req<PrintStamp>(doc.staff ? '/api/staff/print-stamps' : '/api/print-stamps', {
      method: 'POST', quiet401: true,
      body: { kind: doc.kind, ref: doc.ref, title: doc.title ?? null, content_hash: (await contentHash(root)) ?? null, device_id: deviceId() },
    });
    set(stamp);
  } catch (e) {
    set({
      document_code: '—', verification_code: '—', printed_at: new Date().toISOString(), printed_by: '—',
      title: doc.title ?? document.title, verify_url: '',
      unverified: MOCK ? '체험 모드 출력물 — 원본 검증코드가 없어요' : '검증코드를 발급하지 못한 출력물 — 원본으로 확인되지 않아요',
    });
    void e;
  }
  await nextFrame();
  const clear = () => { set(null); window.removeEventListener('afterprint', clear); };
  window.addEventListener('afterprint', clear);
  window.print();
}

/** fixed 요소를 쪽마다 반복하지 않는 엔진(WebKit). 시험용으로 <html data-print-flow="1|0">이 우선한다. */
export function printsFixedOnce(): boolean {
  const forced = document.documentElement.dataset.printFlow;
  if (forced === '1' || forced === '0') return forced === '1';
  const ua = navigator.userAgent;
  // iPhone·iPad: 어떤 브라우저든 WebKit (iPadOS는 Mac처럼 보이므로 터치로 구분)
  if (/iP(hone|ad|od)/.test(ua) || (/Macintosh/.test(ua) && navigator.maxTouchPoints > 1)) return true;
  if (/(Chrome|Chromium|CriOS|Edg|OPR|Firefox|FxiOS)\//.test(ua)) return false;
  return /AppleWebKit\//.test(ua);
}

/** 인쇄 표식을 놓을 자리: 머리말은 body 맨 앞, 꼬리말은 body 맨 끝 (앱 레이아웃의 영향 밖) */
function slot(id: string, first: boolean): HTMLElement {
  let el = document.getElementById(id);
  if (!el) {
    el = document.createElement('div');
    el.id = id;
    el.className = 'aq-print-slot';
    if (first) document.body.insertBefore(el, document.body.firstChild);
    else document.body.appendChild(el);
  }
  return el;
}

const when = (iso: string) => new Date(iso).toLocaleString('ko-KR', { timeZone: 'Asia/Seoul', year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: false }) + ' KST';

/** 인쇄할 때만 보이는 머리말·꼬리말. 앱 루트에 한 번 둔다 (실제로는 body 바로 아래에 찍힌다). */
export function PrintStampHost() {
  const s = usePrintStamp();
  const flow = printsFixedOnce();
  // 쪽 여백을 @page로 줄지(WebKit) body 안쪽 여백으로 줄지(그 밖) — CSS가 이 표시를 본다
  useLayoutEffect(() => {
    document.documentElement.classList.toggle('aq-print-flow', flow);
    // @page는 선택자로 가를 수 없어 WebKit일 때만 쪽 여백을 따로 넣는다(문서 뒤에 두어 여백 0 규칙을 덮는다)
    let page = document.getElementById('aq-print-flow-page');
    if (flow && !page) {
      page = document.createElement('style');
      page.id = 'aq-print-flow-page';
      page.textContent = '@media print{@page{size:A4;margin:12mm 0}}';
      document.head.appendChild(page);
    } else if (!flow) page?.remove();
  }, [flow]);
  const mode = flow ? ' is-flow' : '';
  if (!s) {
    // 브라우저 메뉴(Ctrl+P)로 바로 인쇄한 경우
    return createPortal(
      <div className={`aq-print-stamp is-foot is-missing${mode}`} aria-hidden="true">
        검증코드 없는 출력물 — 화면의 ‘인쇄 · PDF 저장’ 버튼으로 출력해야 문서 식별코드와 원본 검증코드가 들어가요.
      </div>,
      slot('aq-print-foot', false),
    );
  }
  return (
    <>
      {createPortal(
        <div className={`aq-print-stamp is-head${mode}`} aria-hidden="true">
          <span><b>AUDENIQ</b> · {s.title.replace(/^AUDENIQ\s*/, '')}</span>
          <span>문서 식별코드 <code>{s.document_code}</code></span>
        </div>,
        slot('aq-print-head', true),
      )}
      {createPortal(<PrintFoot s={s} mode={mode} />, slot('aq-print-foot', false))}
    </>
  );
}

function PrintFoot({ s, mode }: { s: PrintStamp; mode: string }) {
  return (
    <div className={`aq-print-stamp is-foot${mode}`} aria-hidden="true">
      {s.unverified ? (
        <span className="is-warn">{s.unverified} · 출력 {when(s.printed_at)}</span>
      ) : (
        <>
          <span>원본 검증코드 <code>{s.verification_code}</code> · 출력 {when(s.printed_at)} · 출력자 {s.printed_by}</span>
          <span>진위 확인 {s.verify_url.replace(/^https?:\/\//, '')}</span>
        </>
      )}
    </div>
  );
}
