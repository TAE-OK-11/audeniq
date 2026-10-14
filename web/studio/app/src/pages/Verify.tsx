// 출력물 진위 확인 (/verify) — 로그인 없이, 종이에 찍힌 원본 검증코드로 확인한다.
// 공개 화면에는 출력자를 가린 요약만 보이고, 접속 정보·기기는 AUDENIQ 직원만 볼 수 있다.
import { useEffect, useState } from 'react';
import { Link, useSearchParams } from '../lib/router';
import { req } from '../api/http';
import { errorMessage } from '../api/errors';

interface Result { valid: boolean; document_code?: string; title?: string; printed_at?: string; printed_by?: string; content_hash?: string | null }

export function Verify() {
  const [params] = useSearchParams();
  const [code, setCode] = useState(params.get('code') ?? '');
  const [result, setResult] = useState<Result | null>(null);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);

  async function check(value = code) {
    if (!value.trim() || busy) return;
    setBusy(true);
    setError('');
    try {
      setResult(await req<Result>(`/api/print-stamps/verify?code=${encodeURIComponent(value.trim())}`, { quiet401: true }));
    } catch (e) {
      setError(errorMessage(e, '확인하지 못했어요. 잠시 후 다시 시도해 주세요.'));
    } finally {
      setBusy(false);
    }
  }
  useEffect(() => { if (code) void check(code); }, []); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <div className="aq-terms-page">
      <header className="aq-sign-top">
        <Link to="/" aria-label="AUDENIQ"><img src={`${import.meta.env.BASE_URL}static/AUDENIQ_Logo_Light.svg`} alt="AUDENIQ" /></Link>
        <span>출력물 확인</span>
      </header>
      <article className="aq-terms aq-verify">
        <h1>출력물 진위 확인</h1>
        <p>AUDENIQ에서 인쇄하거나 PDF로 저장한 약관·신청서·계약서의 아래쪽에 찍힌 <b>원본 검증코드</b>를 입력해 주세요.</p>
        <form onSubmit={e => { e.preventDefault(); void check(); }} className="aq-verify-form">
          <label htmlFor="verifyCode" className="sr-only">원본 검증코드</label>
          <input id="verifyCode" value={code} onChange={e => setCode(e.target.value)} placeholder="XXXXX-XXXXX-XXXXX-XXXXX" autoComplete="off" spellCheck={false} maxLength={40} />
          <button className={`ui-btn is-primary${busy ? ' is-busy' : ''}`} type="submit" disabled={busy}>확인</button>
        </form>
        {error && <div className="ui-callout is-danger" role="alert">{error}</div>}
        {result && !result.valid && <div className="ui-callout is-danger" role="status">등록되지 않은 코드예요. 글자를 다시 확인해 주세요. 코드가 맞는데도 나오지 않으면 AUDENIQ에서 출력한 원본이 아니에요.</div>}
        {result?.valid && (
          <div className="ui-callout is-success" role="status">
            <p><b>AUDENIQ에서 출력한 원본이에요.</b> 종이의 내용이 아래와 같은지 확인해 주세요.</p>
            <dl className="aq-verify-dl">
              <dt>문서</dt><dd>{result.title}</dd>
              <dt>문서 식별코드</dt><dd><code>{result.document_code}</code></dd>
              <dt>출력 시각</dt><dd>{result.printed_at && new Date(result.printed_at).toLocaleString('ko-KR', { timeZone: 'Asia/Seoul' })} (KST)</dd>
              <dt>출력자</dt><dd>{result.printed_by}</dd>
              {result.content_hash && <><dt>내용 해시</dt><dd><code>{result.content_hash.slice(0, 16)}…</code></dd></>}
            </dl>
          </div>
        )}
      </article>
    </div>
  );
}
