// 공동 작가 지분 확인 (/split-confirm/:token) — 작가가 보낸 링크. 계정 없이 곡의 참여자·지분을 보고 한 번 답한다.
// 맞으면 '맞아요', 다르면 '달라요'와 의견 — 작가의 곡 화면에 확인 완료 / 이의 제기(의견 그대로)로 보인다.
// 모양은 수익 분배 요청(/splits/:token)과 같은 한 장짜리 화면
import { useEffect, useState } from 'react';
import { Link, useParams } from '../lib/router';
import { pubApi, type SplitInvite } from '../api/publishing';
import { errorMessage } from '../api/errors';
import { pct, roleTextOf } from '../lib/publishing';
import type { ContributorRole } from '../api/publishing';
import { SkeletonLines } from '../components/Skeleton';

export function SplitConfirm() {
  const { token = '' } = useParams<{ token: string }>();
  const [inv, setInv] = useState<SplitInvite | null>(null);
  const [gone, setGone] = useState(false);
  const [disagree, setDisagree] = useState(false);
  const [note, setNote] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [done, setDone] = useState<'' | 'confirmed' | 'disputed'>('');
  useEffect(() => { pubApi.splitInvite(token).then(setInv, () => setGone(true)); }, [token]);
  const you = inv?.people.find(p => p.you);

  const answer = async (agree: boolean) => {
    setBusy(true); setError('');
    try { await pubApi.answerSplitInvite(token, agree, agree ? undefined : note.trim()); setDone(agree ? 'confirmed' : 'disputed'); }
    catch (e) { setError(errorMessage(e, '보내지 못했어요. 다시 시도해 주세요.')); }
    finally { setBusy(false); }
  };

  return (
    <main className="aq-terms-page">
      <header className="aq-sign-top">
        <Link to="/" aria-label="AUDENIQ"><img src={`${import.meta.env.BASE_URL}static/AUDENIQ_Logo_Light.svg`} alt="AUDENIQ" /></Link>
        <span>지분 확인</span>
      </header>
      <article className="aq-terms aq-split-invite">
        {gone ? (
          <><h1>이 링크는 쓸 수 없어요</h1><p className="muted">이미 답했거나, 새 링크를 받아 바뀐 링크예요. 가장 최근에 받은 링크를 열어 주세요.</p></>
        ) : !inv ? <SkeletonLines /> : done ? (
          <div className="ui-callout is-success" role="status">
            {done === 'confirmed' ? <>확인했어요. ‘{inv.title}’에서 내 몫 {you && pct(you.share)}가 그대로 등록돼요.</> : <>의견을 전했어요. {inv.from}에서 지분을 다시 나누면 새 링크를 보내 드려요.</>}
          </div>
        ) : inv.answer ? (
          <p className="muted">이미 답한 요청이에요.</p>
        ) : (
          <>
            <h1>{inv.from}에서 ‘{inv.title}’의 지분 확인을 요청했어요</h1>
            <p className="small muted">공동 작곡 참여자와 각자의 창작 지분이에요. 내 몫이 맞는지 확인해 주세요. AUDENIQ 계정이 없어도 돼요.</p>
            <dl className="ui-kv">
              {inv.people.map(p => (
                <div key={p.name + p.share}>
                  <dt>{p.name}{p.you && ' (나)'}</dt>
                  <dd>{p.you ? <b>{pct(p.share)}</b> : pct(p.share)} · {roleTextOf(p.roles as ContributorRole[])}</dd>
                </div>
              ))}
            </dl>
            {disagree && (
              <div className="ui-field">
                <label htmlFor="scNote">무엇이 다른가요?</label>
                <textarea id="scNote" className="ui-textarea" rows={3} maxLength={1000} value={note} onChange={e => setNote(e.target.value)} placeholder="예: 제 몫은 50%로 합의했어요. 편곡도 함께 했어요." />
              </div>
            )}
            {error && <p className="aq-smartlink-alert-err" role="alert">{error}</p>}
            <div className="aq-confirm-actions">
              {disagree
                ? <><button type="button" className="ui-btn is-soft" disabled={busy} onClick={() => setDisagree(false)}>취소</button>
                    <button type="button" className="ui-btn is-primary" disabled={busy || !note.trim()} onClick={() => void answer(false)}>의견 보내기</button></>
                : <><button type="button" className="ui-btn is-soft" disabled={busy} onClick={() => setDisagree(true)}>달라요</button>
                    <button type="button" className="ui-btn is-primary" disabled={busy} onClick={() => void answer(true)}>맞아요, 확인</button></>}
            </div>
            <p className="small muted">한 번 답하면 이 링크는 끝나요. 모르는 내용이면 답하지 않아도 돼요.</p>
          </>
        )}
      </article>
    </main>
  );
}
