// 권리침해 신고 (/rights-report) — 계정 없이 누구나. AUDENIQ로 배급된 음원이 내 권리를 침해한다고 알릴 때.
// 연락처는 서버에서 바로 암호화해 보관하고, 담당자만 연락에 쓴다 (아티스트에게는 보이지 않는다).
import { useState } from 'react';
import { Link } from '../lib/router';
import { req } from '../api/http';
import { errorMessage } from '../api/errors';
import { MOCK } from '../lib/mode';

const RIGHTS: [string, string][] = [
  ['COPYRIGHT', '저작권 (작사·작곡·편곡 등)'],
  ['NEIGHBORING', '저작인접권 (음원·실연)'],
  ['TRADEMARK', '상표권'],
  ['PUBLICITY', '초상·성명 (퍼블리시티)'],
  ['OTHER', '기타'],
];

export function RightsReport() {
  const [f, setF] = useState({
    name: '', email: '', phone: '', role: 'RIGHTS_HOLDER', right: 'COPYRIGHT', work: '', target: '', urls: '', agree: false,
  });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [receipt, setReceipt] = useState('');
  const set = <K extends keyof typeof f>(k: K, v: (typeof f)[K]) => setF(x => ({ ...x, [k]: v }));

  async function submit() {
    if (busy) return;
    setError('');
    if (!f.name.trim() || !f.email.trim() || !f.work.trim() || !f.target.trim()) {
      setError('이름·이메일·권리 대상 저작물·침해 음원을 모두 적어 주세요.');
      return;
    }
    if (!f.agree) { setError('아래 확인 내용에 동의해 주세요.'); return; }
    const urls = f.urls.split(/\s+/).map(u => u.trim()).filter(Boolean);
    if (urls.length > 5) { setError('증빙 링크는 5개까지 적을 수 있어요.'); return; }
    setBusy(true);
    try {
      if (MOCK) {
        setReceipt('IC-DEMO0001');
      } else {
        const r = await req<{ receipt: string }>('/api/infringement-claims', {
          method: 'POST', quiet401: true,
          body: {
            claimant_name: f.name, claimant_email: f.email, claimant_phone: f.phone, claimant_role: f.role,
            right_type: f.right, work: f.work, target: f.target, evidence_urls: urls, statement_accepted: f.agree,
          },
        });
        setReceipt(r.receipt);
      }
    } catch (e) {
      setError(errorMessage(e, '접수하지 못했어요. 잠시 후 다시 시도해 주세요.'));
    } finally {
      setBusy(false);
    }
  }

  return (
    <main className="aq-terms-page">
      <header className="aq-sign-top">
        <Link to="/" aria-label="AUDENIQ"><img src={`${import.meta.env.BASE_URL}static/AUDENIQ_Logo_Light.svg`} alt="AUDENIQ" /></Link>
        <span>권리침해 신고</span>
      </header>
      <article className="aq-terms aq-rights-report">
        <h1>권리침해 신고</h1>
        {receipt ? (
          <div className="ui-callout is-success" role="status">
            <p><b>신고를 받았어요. 접수 번호는 {receipt}예요.</b></p>
            <p>담당자가 내용을 확인한 뒤 적어 주신 연락처로 연락드려요. 침해가 확인되면 해당 음원의 배급을 중단하고, 배급한 아티스트에게 소명 기회를 줘요.</p>
          </div>
        ) : (
          <form className="aq-report-form" onSubmit={e => { e.preventDefault(); void submit(); }}>
            <p>AUDENIQ를 통해 배급된 음원이 내 저작권·저작인접권 등 권리를 침해한다면 알려 주세요. 담당자가 확인한 뒤 해당 음원의 배급 중단 여부를 정해요.</p>
            <fieldset>
              <legend>신고하는 분</legend>
              <div className="ui-field"><label htmlFor="rrName">이름 또는 회사명</label>
                <input id="rrName" className="ui-input" maxLength={100} value={f.name} onChange={e => set('name', e.target.value)} autoComplete="name" /></div>
              <div className="ui-field"><label htmlFor="rrEmail">이메일</label>
                <input id="rrEmail" className="ui-input" type="email" maxLength={254} value={f.email} onChange={e => set('email', e.target.value)} autoComplete="email" /></div>
              <div className="ui-field"><label htmlFor="rrPhone">전화번호<span className="aq-optional">선택</span></label>
                <input id="rrPhone" className="ui-input" type="tel" maxLength={20} value={f.phone} onChange={e => set('phone', e.target.value)} autoComplete="tel" /></div>
              <div className="ui-field"><label htmlFor="rrRole">신고하는 분은</label>
                <select id="rrRole" className="ui-select" value={f.role} onChange={e => set('role', e.target.value)}>
                  <option value="RIGHTS_HOLDER">권리자 본인</option>
                  <option value="AGENT">권리자의 대리인</option>
                </select></div>
            </fieldset>
            <fieldset>
              <legend>침해 내용</legend>
              <div className="ui-field"><label htmlFor="rrRight">침해된 권리</label>
                <select id="rrRight" className="ui-select" value={f.right} onChange={e => set('right', e.target.value)}>
                  {RIGHTS.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
                </select></div>
              <div className="ui-field"><label htmlFor="rrWork">권리 대상 저작물</label>
                <textarea id="rrWork" className="ui-input" rows={3} maxLength={2000} value={f.work} onChange={e => set('work', e.target.value)} placeholder="곡 제목, 권리자, 발표 시기 등" /></div>
              <div className="ui-field"><label htmlFor="rrTarget">침해하는 음원</label>
                <textarea id="rrTarget" className="ui-input" rows={3} maxLength={2000} value={f.target} onChange={e => set('target', e.target.value)} placeholder="곡 제목·아티스트, 플랫폼 주소, UPC·ISRC를 아시면 함께" /></div>
              <div className="ui-field"><label htmlFor="rrUrls">증빙 링크<span className="aq-optional">선택</span></label>
                <textarea id="rrUrls" className="ui-input" rows={2} value={f.urls} onChange={e => set('urls', e.target.value)} placeholder="https://" aria-describedby="rrUrlsHelp" />
                <p id="rrUrlsHelp" className="ui-field-help">한 줄에 하나씩, 5개까지 적어 주세요.</p></div>
            </fieldset>
            <label className="aq-check-line">
              <input type="checkbox" checked={f.agree} onChange={e => set('agree', e.target.checked)} />
              <span>적은 내용이 사실이며, 신고하는 음원의 이용이 권리자에게 허락받지 않은 것이라고 믿을 만한 이유가 있어요. 사실과 다른 신고로 다른 사람에게 손해를 끼치면 책임질 수 있음을 알고 있어요.</span>
            </label>
            {error && <div className="ui-callout is-danger" role="alert">{error}</div>}
            <button type="submit" className={`ui-btn is-primary is-lg is-block${busy ? ' is-busy' : ''}`} disabled={busy}>신고하기</button>
            <p className="ui-field-help">연락처는 암호화해 보관하고 이 신고의 처리에만 써요. 배급한 아티스트에게는 신고 내용만 전달되고 연락처는 보이지 않아요.</p>
          </form>
        )}
      </article>
    </main>
  );
}
