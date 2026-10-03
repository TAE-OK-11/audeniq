// 권리자 서명 화면 (/sign/:token) — 로그인 없이, 받은 링크로 들어와
// 본인확인 → 문서 확인·동의 → 자필 서명 순서로 진행한다. 서명 후에는 서명본과 서명 기록을 보여 준다.
import { useEffect, useRef, useState } from 'react';
import { useNavigate, useParams, useSearchParams } from '../lib/router';
import { signingApi, type SigningConsents, type SigningView } from '../api/signing';
import { errorMessage } from '../api/errors';
import { SignaturePad, type SignaturePadHandle } from '../components/SignaturePad';
import { compactSignature } from '../lib/application';
import { SIGNING_CONSENTS, SIGNING_PRIVACY_NOTICE, type SigningConsentKey } from '../lib/rightsDocument';
import { CheckIcon } from '../components/Check';
import { Glyph } from '../components/Glyph';

const EVENT_LABEL: Record<string, string> = {
  CREATED: '서명 요청 생성', LINK_REISSUED: '서명 링크 다시 발급', VIEWED: '문서 열람',
  IDENTITY_VERIFIED: '본인확인 완료', IDENTITY_FAILED: '본인확인 실패', SIGNED: '서명 완료',
  DECLINED: '서명 거절', CANCELLED: '요청 취소',
};

function when(iso: string | null | undefined): string {
  if (!iso) return '';
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleString('ko-KR', { dateStyle: 'long', timeStyle: 'medium' });
}

/** 같은 기기에서 서명한 뒤 돌아갈 화면 — 사이트 안 경로만 */
function safeReturn(from: string | null): string {
  return from && from.startsWith('/') && !from.startsWith('//') && !from.startsWith('/sign/') ? from : '';
}

export function Sign() {
  const { token = '' } = useParams();
  const [params] = useSearchParams();
  const nav = useNavigate();
  const back = safeReturn(params.get('from'));
  const [view, setView] = useState<SigningView | null>(null);
  const [loadError, setLoadError] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [name, setName] = useState('');
  const [birth, setBirth] = useState('');
  const [consents, setConsents] = useState<SigningConsents>({ document: false, electronic_signature: false, privacy: false });
  const [readAll, setReadAll] = useState(false);
  const [signed, setSigned] = useState(false);
  const [declining, setDeclining] = useState(false);
  const [reason, setReason] = useState('');
  const pad = useRef<SignaturePadHandle>(null);
  const docRef = useRef<HTMLDivElement>(null);

  const load = async () => {
    try { setView(await signingApi.view(token)); setLoadError(''); }
    catch (e) { setLoadError(errorMessage(e, '서명 링크를 열지 못했어요.')); }
  };
  useEffect(() => { void load(); }, [token]); // eslint-disable-line react-hooks/exhaustive-deps

  // 문서를 끝까지 내려 읽어야 ‘내용 확인’에 동의할 수 있다
  const onScroll = () => {
    const el = docRef.current;
    if (el && el.scrollTop + el.clientHeight >= el.scrollHeight - 24) setReadAll(true);
  };
  useEffect(() => {
    const el = docRef.current;
    if (el && el.scrollHeight <= el.clientHeight + 24) setReadAll(true);
  }, [view?.body]);

  const run = async (fn: () => Promise<void>) => {
    if (busy) return;
    setBusy(true); setError('');
    try { await fn(); } catch (e) { setError(errorMessage(e)); } finally { setBusy(false); }
  };

  const verify = () => run(async () => {
    if (!name.trim() || !/^\d{4}-\d{2}-\d{2}$/.test(birth)) { setError('이름과 생년월일을 입력해 주세요.'); return; }
    await signingApi.verify(token, `test:${name.trim()}:${birth}`);
    await load();
  });

  const sign = () => run(async () => {
    if (!consents.document || !consents.electronic_signature || !consents.privacy) { setError('세 가지 동의에 모두 체크해 주세요.'); return; }
    if (!signed || pad.current?.isEmpty()) { setError('서명란에 직접 서명해 주세요.'); return; }
    const signature = await compactSignature(pad.current?.toDataURL() ?? '');
    await signingApi.sign(token, signature, consents);
    await load();
    window.scrollTo({ top: 0, behavior: 'smooth' });
  });

  const decline = () => run(async () => {
    await signingApi.decline(token, reason);
    setDeclining(false);
    await load();
  });

  const brand = (
    <header className="aq-sign-top">
      <img src={`${import.meta.env.BASE_URL}static/AUDENIQ_Logo_Light.svg`} alt="AUDENIQ" />
      <span>전자 문서 서명</span>
    </header>
  );

  if (loadError || !view) {
    return (
      <div className="aq-sign-page">
        {brand}
        <section className="aq-sign-card aq-sign-closed" aria-busy={!loadError}>
          {loadError ? <><h1>서명 링크를 열 수 없어요</h1><p>{loadError} 링크가 바뀌었거나 기간이 지났을 수 있어요. 서명을 요청한 분께 새 링크를 받아 주세요.</p></>
            : <p className="muted">문서를 불러오고 있어요…</p>}
        </section>
      </div>
    );
  }

  const v = view;
  const closed = v.status === 'DECLINED' || v.status === 'CANCELLED' || v.status === 'EXPIRED';
  const verified = v.status === 'VERIFIED' || v.status === 'SIGNED';
  const step = v.status === 'SIGNED' ? 3 : verified ? 2 : 1;
  const demoProvider = v.identity_provider.name === 'test' || v.identity_provider.name === 'demo';

  if (closed) {
    const msg = v.status === 'DECLINED' ? ['서명을 거절했어요', '요청한 분께 거절 사실이 전달돼요. 내용을 고쳐 다시 요청하면 새 링크로 받게 돼요.']
      : v.status === 'CANCELLED' ? ['요청이 취소됐어요', '서명을 요청한 분이 이 요청을 취소했어요. 이 링크로는 서명할 수 없어요.']
        : ['서명 링크 기간이 지났어요', '보안을 위해 서명 링크는 정해진 기간에만 쓸 수 있어요. 요청한 분께 새 링크를 받아 주세요.'];
    return (
      <div className="aq-sign-page">
        {brand}
        <section className="aq-sign-card aq-sign-closed"><h1>{msg[0]}</h1><p>{msg[1]}</p>
          {back && <button type="button" className="button secondary" onClick={() => nav(back)}>돌아가기</button>}
        </section>
      </div>
    );
  }

  return (
    <div className="aq-sign-page">
      {brand}
      <section className="aq-sign-card aq-sign-intro">
        <p className="eyebrow">{v.form}</p>
        <h1>{v.title}</h1>
        <p className="aq-sign-lead">
          {v.requested_by || v.artist || '발매 신청인'}님이 <b>{v.signer_name}</b>님께 서명을 요청했어요.
          {v.status !== 'SIGNED' && <> 본인확인을 마친 뒤 문서를 읽고 직접 서명해 주세요.</>}
        </p>
        <dl className="aq-sign-facts">
          <div><dt>권리자</dt><dd>{v.rights_holder}</dd></div>
          <div><dt>서명자</dt><dd>{v.signer_name} · {v.signer_role}</dd></div>
          {v.release_title && <div><dt>발매</dt><dd>{v.release_title}</dd></div>}
          <div><dt>{v.status === 'SIGNED' ? '서명 시각' : '서명 기한'}</dt><dd>{when(v.status === 'SIGNED' ? v.signed_at : v.expires_at)}</dd></div>
        </dl>
        <ol className="aq-sign-steps" aria-label="서명 단계">
          {['본인확인', '문서 확인·동의', '서명'].map((label, i) => (
            <li key={label} className={i + 1 < step || v.status === 'SIGNED' ? 'is-done' : i + 1 === step ? 'is-current' : ''}>
              <span aria-hidden="true">{i + 1 < step || v.status === 'SIGNED' ? <CheckIcon size={11} /> : i + 1}</span>{label}
            </li>
          ))}
        </ol>
      </section>

      {v.status === 'SIGNED' ? (
        <section className="aq-sign-card aq-sign-done" aria-labelledby="aqSignDone">
          <span className="aq-sign-done-mark" aria-hidden="true"><CheckIcon size={22} /></span>
          <h2 id="aqSignDone">서명이 끝났어요</h2>
          <p>서명본과 서명 기록을 아래에서 확인할 수 있어요. 필요하면 저장하거나 인쇄해 두세요. 이 링크로 서명 후 90일 동안 다시 볼 수 있어요.</p>
          <div className="aq-sign-actions">
            <button type="button" className="button secondary" onClick={() => window.print()}><Glyph name="download" size={15} /> 저장·인쇄</button>
            {back && <button type="button" className="button" onClick={() => nav(back)}>발매 신청인에게 기기 돌려주기</button>}
          </div>
        </section>
      ) : (
        <section className="aq-sign-card" aria-labelledby="aqSignStep1">
          <h2 id="aqSignStep1"><span className="aq-sign-no">1</span>본인확인</h2>
          {verified ? (
            <p className="aq-sign-ok"><span className="aq-check-badge is-sm"><CheckIcon size={10} /></span>
              {v.identity?.name}님 본인확인 완료 · {when(v.identity?.verified_at)}</p>
          ) : !v.identity_provider.ready ? (
            <div className="notice">
              본인확인 서비스(휴대폰·간편인증)를 연결하고 있어요. 연결되기 전에는 법적 효력이 있는 전자 서명을 할 수 없어요.
              급하면 서면 허락서에 서명해 요청한 분께 전달해 주세요.
            </div>
          ) : demoProvider ? (
            <>
              <p className="small muted">체험·테스트 환경이에요. 실제 서비스에서는 휴대폰·간편인증(PASS·카카오·네이버·토스) 창이 열려요.</p>
              <div className="aq-sign-grid">
                <div className="field"><label htmlFor="aqSignName">이름</label><input id="aqSignName" autoComplete="name" value={name} onChange={e => setName(e.target.value)} placeholder={v.signer_name} /></div>
                <div className="field"><label htmlFor="aqSignBirth">생년월일</label><input id="aqSignBirth" type="date" value={birth} onChange={e => setBirth(e.target.value)} /></div>
              </div>
              <button type="button" className="button studio-submit-wide" disabled={busy} onClick={() => void verify()}>{busy ? '확인 중…' : '본인확인하기'}</button>
            </>
          ) : (
            <div className="notice">본인확인 창을 여는 연결이 아직 준비되지 않았어요.</div>
          )}
        </section>
      )}

      <section className="aq-sign-card" aria-labelledby="aqSignStep2">
        <h2 id="aqSignStep2">{v.status !== 'SIGNED' && <span className="aq-sign-no">2</span>}{v.status === 'SIGNED' ? '서명본' : '문서 확인·동의'}</h2>
        <div className={`aq-document-snapshot aq-sign-doc${v.status === 'SIGNED' ? ' is-signed' : ''}`} ref={docRef} onScroll={onScroll} tabIndex={0} aria-label="문서 내용">
          {v.body}
          {v.status === 'SIGNED' && v.signature && (
            <div className="aq-sign-signature">
              <span>서명자 {v.signer_name} ({v.signer_role})</span>
              <img src={v.signature} alt={`${v.signer_name} 서명`} />
              <span>{when(v.signed_at)}</span>
            </div>
          )}
        </div>
        {v.status !== 'SIGNED' && verified && (
          <div className="aq-sign-consents">
            {!readAll && <p className="small muted">문서를 끝까지 내려 읽으면 동의할 수 있어요.</p>}
            {(Object.keys(SIGNING_CONSENTS) as SigningConsentKey[]).map(k => (
              <label key={k} className="check-line">
                <input type="checkbox" checked={consents[k]} disabled={busy || (k === 'document' && !readAll)}
                  onChange={e => setConsents(c => ({ ...c, [k]: e.target.checked }))} />
                <span><b>(필수)</b> {SIGNING_CONSENTS[k]}</span>
              </label>
            ))}
            <details className="aq-sign-privacy">
              <summary>개인정보 수집·이용 안내 보기</summary>
              <ul>{SIGNING_PRIVACY_NOTICE.map(line => <li key={line}>{line}</li>)}</ul>
            </details>
          </div>
        )}
      </section>

      {v.status === 'VERIFIED' && (
        <section className="aq-sign-card" aria-labelledby="aqSignStep3">
          <h2 id="aqSignStep3"><span className="aq-sign-no">3</span>직접 서명</h2>
          <p className="small muted">{v.signer_name}님이 손가락이나 마우스로 직접 서명해 주세요. 서명하면 문서 내용은 더 이상 바꿀 수 없어요.</p>
          <div className={busy ? 'aq-rights-pad is-busy' : 'aq-rights-pad'} inert={busy}>
            <SignaturePad ref={pad} id="aqSignPad" label={`${v.signer_name} 서명`} onChange={setSigned} />
          </div>
          <button type="button" className="button studio-submit-wide" disabled={busy} onClick={() => void sign()}>{busy ? '서명하는 중…' : '동의하고 서명하기'}</button>
        </section>
      )}

      {v.status === 'SIGNED' && (
        <section className="aq-sign-card" aria-labelledby="aqSignLog">
          <h2 id="aqSignLog">서명 기록</h2>
          <dl className="aq-sign-facts">
            <div><dt>본인확인</dt><dd>{v.identity?.name} · {when(v.identity?.verified_at)}</dd></div>
            <div><dt>문서번호</dt><dd className="aq-sign-mono">{v.document_no}</dd></div>
            <div><dt>문서 해시</dt><dd className="aq-sign-mono">{v.body_hash}</dd></div>
            <div><dt>서명 증명 해시</dt><dd className="aq-sign-mono">{v.certificate_hash}</dd></div>
          </dl>
          <ol className="aq-sign-log">
            {v.events.map(e => (
              <li key={e.hash}><strong>{EVENT_LABEL[e.event] ?? e.event}</strong><span>{when(e.at)}</span><code>{e.hash.slice(0, 16)}…</code></li>
            ))}
          </ol>
          <p className="small muted">각 기록은 앞 기록의 해시를 이어 받아 만들어져, 나중에 바뀌면 바로 드러나요.</p>
        </section>
      )}

      {error && <p role="alert" className="aq-rights-error aq-sign-error">{error}</p>}

      {v.status !== 'SIGNED' && (
        <section className="aq-sign-decline">
          {declining ? (
            <div className="aq-sign-card">
              <h2>서명하지 않을래요</h2>
              <div className="field"><label htmlFor="aqSignReason">이유 (선택)</label><textarea id="aqSignReason" rows={3} maxLength={500} value={reason} onChange={e => setReason(e.target.value)} placeholder="예: 지분 비율이 실제와 달라요" /></div>
              <div className="aq-sign-actions">
                <button type="button" className="button secondary" disabled={busy} onClick={() => setDeclining(false)}>돌아가기</button>
                <button type="button" className="button danger" disabled={busy} onClick={() => void decline()}>서명 거절하기</button>
              </div>
            </div>
          ) : (
            <button type="button" className="link-btn" onClick={() => setDeclining(true)}>내용이 다르거나 서명하지 않으려면</button>
          )}
        </section>
      )}
    </div>
  );
}
