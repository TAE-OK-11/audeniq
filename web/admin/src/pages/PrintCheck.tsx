// 출력물 확인 (스태프) — 종이·PDF의 원본 검증코드로 누가·언제·어느 기기·어디서 출력했는지 본다.
// 조회 자체가 스태프 접근 기록에 남는다.
import { useState } from 'react';
import { req } from '../api/http';
import { errorMessage } from '../api/errors';

interface Detail {
  valid: boolean; document_code?: string; title?: string; kind?: string; doc_ref?: string; content_hash?: string | null;
  printed_at?: string; printed_by?: string; user_id?: string | null; org_id?: string | null; signing_request_id?: string | null;
  session_id?: string | null; device_id?: string | null; source?: { ip?: string | null; user_agent?: string | null } | null;
  prints_of_this_document?: number;
}

const KIND: Record<string, string> = {
  TERMS: '이용약관', SETTLEMENT_TERMS: '정산 약관', PRIVACY: '개인정보 처리방침', APPLICATION: '배급 신청서',
  DOCUMENT: '계약서·권리 서류', SIGNING: '서명 요청 문서', STAFF_SHEET: '심사 기록(스태프)',
};

export function PrintCheck() {
  const [code, setCode] = useState('');
  const [d, setD] = useState<Detail | null>(null);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);

  async function check(e: React.FormEvent) {
    e.preventDefault();
    if (!code.trim() || busy) return;
    setBusy(true);
    setError('');
    try {
      setD(await req<Detail>(`/api/staff/print-stamps/verify?code=${encodeURIComponent(code.trim())}`));
    } catch (err) {
      setError(errorMessage(err, '확인하지 못했어요.'));
    } finally {
      setBusy(false);
    }
  }

  const row = (k: string, v?: string | number | null) => v != null && v !== '' && (<><dt>{k}</dt><dd>{v}</dd></>);
  return (
    <div className="adm-page">
      <div className="ui-page-head"><div><h1>출력물 확인</h1><p>인쇄물·PDF 아래쪽의 원본 검증코드로 출력 기록 전체를 확인해요.</p></div></div>
      <form onSubmit={check} className="adm-print-check">
        <label htmlFor="pcCode" className="sr-only">원본 검증코드</label>
        <input id="pcCode" value={code} onChange={e => setCode(e.target.value)} placeholder="XXXXX-XXXXX-XXXXX-XXXXX" autoComplete="off" spellCheck={false} maxLength={40} />
        <button className={`ui-btn is-primary${busy ? ' is-busy' : ''}`} type="submit" disabled={busy}>확인</button>
      </form>
      {error && <div className="ui-callout is-danger" role="alert">{error}</div>}
      {d && !d.valid && <div className="ui-callout is-danger" role="status">등록되지 않은 코드예요. AUDENIQ에서 출력한 원본이 아니거나 코드를 잘못 읽었어요.</div>}
      {d?.valid && (
        <dl className="adm-print-detail">
          {row('문서', d.title)}
          {row('종류', d.kind ? KIND[d.kind] ?? d.kind : null)}
          {row('문서 식별코드', d.document_code)}
          {row('이 문서 출력 횟수', d.prints_of_this_document)}
          {row('출력 시각', d.printed_at && new Date(d.printed_at).toLocaleString('ko-KR', { timeZone: 'Asia/Seoul' }) + ' KST')}
          {row('출력자', d.printed_by)}
          {row('사용자 id', d.user_id)}
          {row('작업 공간 id', d.org_id)}
          {row('서명 요청 id', d.signing_request_id)}
          {row('세션 id', d.session_id)}
          {row('기기 id', d.device_id)}
          {row('IP', d.source?.ip)}
          {row('브라우저', d.source?.user_agent)}
          {row('문서 참조', d.doc_ref)}
          {row('내용 해시', d.content_hash)}
        </dl>
      )}
    </div>
  );
}
