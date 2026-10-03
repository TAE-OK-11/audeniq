// 배급 계약서 최종본 (/contracts/:id) — 서버가 만든 계약서 본문을 신청서와 같은 정식 서류로 보여 주고 인쇄·PDF로 저장한다.
import { useEffect, useMemo, useState } from 'react';
import { Link, useNavigate, useParams } from '../lib/router';
import { useDocs, setDocs, docState } from '../store/docs';
import { fetchDocuments } from '../api/portal';
import { MOCK } from '../lib/mode';
import { errorMessage } from '../api/errors';
import { localStamp } from '../lib/format';
import { parseStamp } from '../lib/date';
import { hashLabel } from '../lib/application';
import { SkeletonBlock } from '../components/Skeleton';
import { CheckIcon } from '../components/Check';
import { Glyph } from '../components/Glyph';
import { parseAgreement, type Block } from '../lib/agreementDoc';

function longDate(stamp: string): string {
  const d = parseStamp(stamp);
  return d ? `${d.getFullYear()}년 ${d.getMonth() + 1}월 ${d.getDate()}일` : stamp;
}

export function Agreement() {
  const { id = '' } = useParams();
  const nav = useNavigate();
  const doc = useDocs().find(d => d.id === id && d.kind === 'agreements');
  const [loading, setLoading] = useState(!MOCK);
  const [error, setError] = useState('');
  useEffect(() => {
    if (MOCK) return;
    let live = true;
    fetchDocuments().then(list => { if (live) setDocs(list); })
      .catch(e => { if (live) setError(errorMessage(e)); })
      .finally(() => { if (live) setLoading(false); });
    return () => { live = false; };
  }, [id]);
  const parsed = useMemo(() => parseAgreement(doc?.content ?? ''), [doc?.content]);

  if (loading && !doc) return <div className="view aq-doc-view"><SkeletonBlock height={640} /></div>;
  if (error || !doc) {
    return (
      <div className="empty-page">
        <h2>계약서를 확인할 수 없어요.</h2>
        <p>{error || '이 계정에서 볼 수 있는 계약서가 없어요.'}</p>
        <button type="button" className="button secondary" onClick={() => nav('/contracts')}>계약서 목록</button>
      </div>
    );
  }

  const signed = !!doc.localSignatureAt;
  const terms = doc.agreementTerms;
  const checked = (text: string) => doc.confirmations?.items.find(i => i.text === text)?.checked;
  const docNo = `AUD-DIST-${doc.id.replace(/[^0-9a-z]/gi, '').slice(0, 8).toUpperCase()}`;
  const meta: [string, string][] = [
    ['계약서 번호', docNo],
    ...parsed.meta.filter(([k]) => k !== '적용 약관'),
    ['체결일', signed ? localStamp(doc.localSignatureAt) : '서명 전'],
  ];
  const termsRef = parsed.meta.find(([k]) => k === '적용 약관')?.[1];

  const renderBlocks = (blocks: Block[]) => blocks.map((b, i) => {
    switch (b.kind) {
      case 'kv': return (
        <table key={i} className="aq-paper-kv"><tbody>
          {b.rows.map(([k, v]) => <tr key={k}><th>{k}</th><td>{v}</td></tr>)}
        </tbody></table>
      );
      case 'list': return <ol key={i} className="aq-paper-tracks">{b.items.map(t => <li key={t}>{t}</li>)}</ol>;
      case 'check': return (
        <ol key={i} className="aq-paper-agree">
          {b.items.map(it => {
            const on = signed ? !!checked(it.text) : false;
            return (
              <li key={it.text} className={on ? 'is-on' : ''}>
                <span className="aq-paper-check" aria-label={on ? '확인함' : '확인 전'}>{on ? <CheckIcon size={12} /> : null}</span>
                <span><b className="aq-paper-req">{it.required ? '필수' : '해당 시'}</b> {it.text}</span>
              </li>
            );
          })}
        </ol>
      );
      default: return <p key={i} className="aq-paper-p">{b.text}</p>;
    }
  });

  return (
    <div id="view-agreement" className="view aq-doc-view">
      {!signed && (
        <div className="aq-doc-done is-pending" role="status">
          <span className="aq-doc-done-mark" aria-hidden="true"><Glyph name="pencil" size={18} /></span>
          <div className="min-0">
            <strong>{doc.reviewStatus === 'approved' ? '서명 전 계약서예요.' : '담당자 검토 중인 계약서예요.'}</strong>
            <span>{doc.reviewStatus === 'approved' ? '내용을 확인하고 권리 확인 항목에 체크한 뒤 서명하면 계약이 체결돼요.' : '검토가 끝나면 계약 조건이 확정되고 서명할 수 있어요.'}</span>
          </div>
          {doc.reviewStatus === 'approved' && <button type="button" className="button" onClick={() => nav(`/contracts?doc=${encodeURIComponent(doc.id)}`)}>확인하고 서명</button>}
        </div>
      )}

      <div className="aq-doc-toolbar">
        <button type="button" className="link-btn aq-back-link" onClick={() => nav('/contracts')}><Glyph name="arrow-left" size={15} className="aq-glyph-lead" />계약서</button>
        <div className="row-actions">
          {doc.releaseId && <button type="button" className="button secondary" onClick={() => nav(`/releases/${doc.releaseId}/application`)}>신청서 보기</button>}
          <button type="button" className="button" onClick={() => window.print()}>인쇄 · PDF 저장</button>
        </div>
      </div>

      <article className="aq-paper" aria-label="음원 배급 계약서">
        <header className="aq-paper-head">
          <div className="aq-paper-brand">
            <img src={`${import.meta.env.BASE_URL}static/AUDENIQ_Logo_Light.svg`} alt="AUDENIQ" />
            <dl className="aq-paper-meta">
              {meta.map(([k, v]) => <div key={k}><dt>{k}</dt><dd>{v}</dd></div>)}
            </dl>
          </div>
          <h1>음원 배급 계약서</h1>
          <p className="aq-paper-en">Digital Music Distribution Agreement</p>
        </header>

        {parsed.intro.map(t => <p key={t} className="aq-paper-lead">{t}</p>)}

        {parsed.articles.map(a => (
          <section key={a.no} className="aq-paper-sec">
            <h2><span>{a.no}</span>{a.title}</h2>
            {renderBlocks(a.blocks)}
          </section>
        ))}

        {parsed.annexes.map(a => (
          <section key={a.no} className="aq-paper-sec aq-paper-annex">
            <h2><em>{a.no}</em>{a.title}</h2>
            {renderBlocks(a.blocks)}
          </section>
        ))}

        <section className="aq-paper-signoff">
          <p>위 계약의 내용을 확인하고 체결합니다.</p>
          <p className="aq-paper-date">{signed ? longDate(doc.localSignatureAt) : '서명 전'}</p>
          <div className="aq-paper-parties">
            <div className="aq-paper-signer">
              <span>회사</span>
              <strong>주식회사 AUDENIQ</strong>
              <span className="aq-paper-sig"><em>{doc.approvedAt ? `담당자 승인 · ${longDate(doc.approvedAt)}` : '담당자 승인 전'}</em></span>
            </div>
            <div className="aq-paper-signer">
              <span>이용자</span>
              <strong>{doc.signerName || '—'}</strong>
              <span className="aq-paper-sig">
                {doc.localSignatureData ? <img src={doc.localSignatureData} alt={`${doc.signerName} 서명`} /> : <em>(서명)</em>}
              </span>
            </div>
          </div>
        </section>

        <footer className="aq-paper-foot">
          <div>
            <span>문서 확인 코드 (SHA-256)</span>
            <code>{doc.confirmations?.content_hash ? hashLabel(doc.confirmations.content_hash) : '서명하면 발급돼요'}</code>
          </div>
          <span className={`aq-paper-integrity is-${signed ? 'ok' : 'checking'}`}>{docState(doc)}</span>
          <p>
            이 계약서는 AUDENIQ STUDIO에서 전자문서로 작성되고 전자서명으로 체결돼요.
            {terms && <> 배급수수료 회사 {terms.fee_bps / 100}% · 이용자 {terms.user_bps / 100}%, {terms.exclusivity === 'EXCLUSIVE' ? '독점' : '비독점'} 배급. </>}
            문서 확인 코드는 계약서 본문·계약 조건·확인 항목·서명으로 계산돼, 내용이 바뀌면 달라져요.
            {termsRef && <> 이 계약에서 정하지 않은 사항은 <Link to="/terms">{termsRef}</Link>을 따라요.</>}
          </p>
        </footer>
      </article>
    </div>
  );
}
