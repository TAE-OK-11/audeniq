// 출력물 식별 — 인쇄·PDF 저장을 누르면 서버가 문서 식별코드와 원본 검증코드를 발급하고,
// 모든 쪽의 위·아래에 찍은 뒤 인쇄한다(crates/core/src/print_stamp.rs).
// 브라우저가 넣던 머리말·꼬리말(시각·URL)은 @page 여백을 0으로 두어 빼고, 그 자리에 이 표식을 넣는다.
import { useSyncExternalStore } from 'react';
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
      method: 'POST', quiet401: true, quietServer: true,
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

const when = (iso: string) => new Date(iso).toLocaleString('ko-KR', { timeZone: 'Asia/Seoul', year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: false }) + ' KST';

/** 인쇄할 때만 보이는 머리말·꼬리말 (모든 쪽에 반복). 앱 루트에 한 번 둔다. */
export function PrintStampHost() {
  const s = usePrintStamp();
  if (!s) {
    // 브라우저 메뉴(Ctrl+P)로 바로 인쇄한 경우
    return (
      <div className="aq-print-stamp is-foot is-missing" aria-hidden="true">
        검증코드 없는 출력물 — 화면의 ‘인쇄 · PDF 저장’ 버튼으로 출력해야 문서 식별코드와 원본 검증코드가 들어가요.
      </div>
    );
  }
  // 유출 추적용 워터마크: 출력자와 원본 검증코드를 쪽 전체에 옅게 반복한다
  const mark = s.unverified ? '검증되지 않은 출력물' : `${s.printed_by} · ${s.verification_code}`;
  return (
    <>
      <div className="aq-print-watermark" aria-hidden="true">
        {Array.from({ length: 24 }, (_, i) => <span key={i}>{mark}</span>)}
      </div>
      <div className="aq-print-stamp is-head" aria-hidden="true">
        <span><b>AUDENIQ</b> · {s.title.replace(/^AUDENIQ\s*/, '')}</span>
        <span>문서 식별코드 <code>{s.document_code}</code></span>
      </div>
      <div className="aq-print-stamp is-foot" aria-hidden="true">
        {s.unverified ? (
          <span className="is-warn">{s.unverified} · 출력 {when(s.printed_at)}</span>
        ) : (
          <>
            <span>원본 검증코드 <code>{s.verification_code}</code> · 출력 {when(s.printed_at)} · 출력자 {s.printed_by}</span>
            <span>진위 확인 {s.verify_url.replace(/^https?:\/\//, '')}</span>
          </>
        )}
      </div>
    </>
  );
}
