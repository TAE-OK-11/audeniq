// 실명인증 (스태프) — 현장·고객센터에서 신분증을 직접 보고 계정의 실명·생년월일을 등록한다.
// 신분증 번호는 입력받지 않는다. 처리 기록은 담당자 계정으로 남고, 사용자에게 알림이 간다.
// 서버: crates/core/src/identity_desk.rs (ADMIN·SUPPORT만)
import { useState } from 'react';
import { useConfirm } from '../components/Confirm';
import { useToast } from '../components/Toast';
import { errorMessage } from '../api/errors';
import { staffApi, type GuardianRelation, type IdentityAccount, type IdentityChannel, type IdentityDocument } from '../api/staff';
import { NoDuty, useStaff } from '../staffUi';
import { when } from '../labels';

export const CHANNEL_LABEL: Record<IdentityChannel, string> = { IN_PERSON: '현장', CUSTOMER_CENTER: '고객센터' };
export const DOCUMENT_LABEL: Record<IdentityDocument, string> = {
  RESIDENT_CARD: '주민등록증', DRIVER_LICENSE: '운전면허증', PASSPORT: '여권',
  ALIEN_REGISTRATION: '외국인등록증', DISABILITY_CARD: '장애인등록증', VETERAN_CARD: '국가보훈등록증',
  YOUTH_CARD: '청소년증', FAMILY_RELATION_CERT: '가족관계증명서 + 법정대리인 신분증',
};
/** 미성년자만 고를 수 있는 서류 (서버 identity_desk::MINOR_DOCUMENT_TYPES) */
const MINOR_DOCUMENTS: IdentityDocument[] = ['YOUTH_CARD', 'FAMILY_RELATION_CERT'];
export const GUARDIAN_LABEL: Record<GuardianRelation, string> = { PARENT: '부모', GUARDIAN: '후견인' };
/** 성인 나이 (서버 account::ADULT_AGE, 만 19세) */
const ADULT_AGE = 19;
const METHOD_LABEL: Record<string, string> = { ...CHANNEL_LABEL, pass: 'PASS', sms: '휴대폰', ipin: '아이핀', card: '카드사' };

/** 숫자 8자리 → YYYY-MM-DD (입력하면서 하이픈을 넣는다) */
export function formatBirth(raw: string): string {
  const d = raw.replace(/\D/g, '').slice(0, 8);
  return d.length > 6 ? `${d.slice(0, 4)}-${d.slice(4, 6)}-${d.slice(6)}` : d.length > 4 ? `${d.slice(0, 4)}-${d.slice(4)}` : d;
}
export const validBirth = (v: string) => {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(v)) return false;
  const d = new Date(`${v}T00:00:00Z`);
  return !Number.isNaN(d.getTime()) && d.toISOString().slice(0, 10) === v && d.getTime() < Date.now();
};
/** 오늘 기준 만 나이 (YYYY-MM-DD) */
export function ageOn(birth: string, today = new Date()): number {
  const [y, m, d] = birth.split('-').map(Number);
  const before = today.getMonth() + 1 < m || (today.getMonth() + 1 === m && today.getDate() < d);
  return today.getFullYear() - y - (before ? 1 : 0);
}

const EMPTY = { name: '', birth: '', channel: 'CUSTOMER_CENTER' as IdentityChannel, document: 'RESIDENT_CARD' as IdentityDocument, guardian: '' as GuardianRelation | '', note: '', seen: false };

export function IdentityDesk() {
  const { me, can } = useStaff();
  const toast = useToast();
  const confirm = useConfirm();
  const [email, setEmail] = useState('');
  const [account, setAccount] = useState<IdentityAccount | null>(null);
  const [f, setF] = useState(EMPTY);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const set = <K extends keyof typeof EMPTY>(k: K, v: (typeof EMPTY)[K]) => { setF(p => ({ ...p, [k]: v })); setError(''); };
  // 생년월일이 성인이면 미성년자 서류·법정대리인을 비운다
  const setBirth = (v: string) => {
    setError('');
    setF(p => validBirth(v) && ageOn(v) >= ADULT_AGE
      ? { ...p, birth: v, guardian: '', document: MINOR_DOCUMENTS.includes(p.document) ? 'RESIDENT_CARD' : p.document }
      : { ...p, birth: v });
  };

  if (!can('IDENTITY')) return <div className="adm-page"><NoDuty duty="실명인증" /></div>;

  const isAdmin = me.role === 'ADMIN';
  const current = account?.identity ?? null;
  // 직원 인증을 다른 사람으로 바로잡을 수 있는 경우 (ADMIN만, 사유 필수)
  const canCorrect = isAdmin && !!current?.offline;
  const age = validBirth(f.birth) ? ageOn(f.birth) : null;
  const minor = age !== null && age < ADULT_AGE;
  const documents = (Object.keys(DOCUMENT_LABEL) as IdentityDocument[]).filter(d => minor || !MINOR_DOCUMENTS.includes(d) || d === f.document);

  async function find(e: React.FormEvent) {
    e.preventDefault();
    if (!email.trim() || busy) return;
    setBusy(true); setError(''); setAccount(null); setF(EMPTY);
    try {
      setAccount(await staffApi.identityLookup(email.trim()));
    } catch (err) {
      setError(errorMessage(err, '계정을 찾지 못했어요.'));
    } finally {
      setBusy(false);
    }
  }

  async function save(correction: boolean) {
    if (!account || busy) return;
    if (!f.name.trim()) { setError('신분증에 적힌 실명을 입력해 주세요.'); return; }
    if (!validBirth(f.birth)) { setError('생년월일을 YYYY-MM-DD로 확인해 주세요.'); return; }
    if (!f.seen) { setError('신분증 사진과 본인 얼굴을 대조했는지 확인해 주세요.'); return; }
    if (!minor && MINOR_DOCUMENTS.includes(f.document)) { setError('청소년증·가족관계증명서는 미성년자만 쓸 수 있어요.'); return; }
    if (f.document === 'FAMILY_RELATION_CERT' && !f.guardian) { setError('가족관계증명서로 확인할 때는 함께 온 법정대리인을 골라 주세요.'); return; }
    if (correction && !f.note.trim()) { setError('바로잡는 사유를 적어 주세요.'); return; }
    const ok = await confirm({
      title: correction ? '실명 정보를 바로잡을까요?' : '실명인증을 저장할까요?',
      message: `${account.email_masked} 계정을 ${f.name.trim()} (${f.birth})으로 ${correction ? '바로잡아요' : '인증해요'}. ${CHANNEL_LABEL[f.channel]} · ${DOCUMENT_LABEL[f.document]}${minor ? ` · 미성년(만 ${age}세)${f.guardian ? ` · 법정대리인(${GUARDIAN_LABEL[f.guardian]}) 동행` : ''}` : ''}. 처리 기록이 내 계정으로 남고, 사용자에게 알림이 가요.`,
      confirmLabel: correction ? '바로잡기' : '인증 저장',
    });
    if (!ok) return;
    setBusy(true); setError('');
    try {
      const next = await staffApi.identityVerify(account.user_id, {
        real_name: f.name.trim(), birth_date: f.birth, channel: f.channel, document_type: f.document,
        guardian_relation: minor && f.guardian ? f.guardian : null,
        note: f.note.trim(), correction,
      });
      setAccount(next);
      setF(EMPTY);
      toast(correction ? '실명 정보를 바로잡았어요.' : '실명인증을 저장했어요.', 'success');
    } catch (err) {
      setError(errorMessage(err, '저장하지 못했어요.'));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="adm-page">
      <div className="ui-page-head"><div>
        <h1>실명인증</h1>
        <p>현장이나 고객센터에서 신분증 원본을 직접 보고 실명·생년월일을 등록해요. 신분증 번호는 적지 않아요.</p>
      </div></div>

      <form onSubmit={find} className="adm-id-find">
        <label htmlFor="idEmail" className="sr-only">가입 이메일</label>
        <input id="idEmail" type="email" className="ui-input" value={email} onChange={e => setEmail(e.target.value)}
          placeholder="가입 이메일" autoComplete="off" spellCheck={false} maxLength={254} />
        <button className={`ui-btn is-primary${busy && !account ? ' is-busy' : ''}`} type="submit" disabled={busy}>계정 찾기</button>
      </form>
      {error && <div className="ui-callout is-danger" role="alert">{error}</div>}

      {account && (
        <>
          <dl className="adm-print-detail">
            <dt>계정</dt><dd>{account.email_masked}</dd>
            <dt>사용자 id</dt><dd>{account.user_id}</dd>
            <dt>상태</dt><dd>{account.status === 'ACTIVE' ? '사용 중' : account.status}</dd>
            <dt>가입</dt><dd>{when(account.joined_at)}</dd>
            <dt>실명인증</dt>
            <dd>{current
              ? `${current.name_masked} · ${current.offline ? '직원 확인' : '본인인증'}(${METHOD_LABEL[current.method] ?? current.method}) · ${when(current.verified_at)} · ${current.adult ? '성인' : '미성년'}`
              : '아직 안 했어요'}</dd>
          </dl>

          {account.status !== 'ACTIVE' ? (
            <div className="ui-callout">사용 중인 계정만 인증할 수 있어요.</div>
          ) : (
            <div className="ui-card adm-id-form">
              <h2 className="ui-group-title">{current ? '다시 확인' : '신분증으로 인증'}</h2>
              {current && <p className="ui-field-help">이미 인증된 계정이에요. 같은 사람이면 확인 기록만 남아요{canCorrect ? '. 직원이 잘못 입력한 경우에만 아래에서 바로잡을 수 있어요.' : '. 다른 사람이면 저장되지 않아요.'}</p>}
              <div className="adm-id-grid">
                <div className="ui-field"><label htmlFor="idName">실명 <span className="required">*</span></label>
                  <input id="idName" className="ui-input" maxLength={120} value={f.name} onChange={e => set('name', e.target.value)} placeholder="신분증에 적힌 그대로" autoComplete="off" /></div>
                <div className="ui-field"><label htmlFor="idBirth">생년월일 <span className="required">*</span></label>
                  <input id="idBirth" className="ui-input" inputMode="numeric" value={f.birth} onChange={e => setBirth(formatBirth(e.target.value))} placeholder="YYYY-MM-DD" autoComplete="off" /></div>
              </div>
              {minor && (
                <div className="ui-callout adm-id-minor">
                  만 {age}세 미성년자예요. 주민등록증이 없으면 <b>청소년증</b>이나, 함께 온 법정대리인의 신분증과
                  <b> 가족관계증명서</b>로 확인해요. 법정대리인의 이름·신분증 번호는 적지 않아요.
                  발매·계약에는 따로 법정대리인 동의가 필요해요.
                </div>
              )}
              <div className="adm-id-grid">
                <div className="ui-field"><label htmlFor="idChannel">확인한 곳</label>
                  <select id="idChannel" className="ui-select" value={f.channel} onChange={e => set('channel', e.target.value as IdentityChannel)}>
                    {Object.entries(CHANNEL_LABEL).map(([v, l]) => <option key={v} value={v}>{l}</option>)}
                  </select></div>
                <div className="ui-field"><label htmlFor="idDoc">신분증</label>
                  <select id="idDoc" className="ui-select" value={f.document} onChange={e => set('document', e.target.value as IdentityDocument)}>
                    {documents.map(v => <option key={v} value={v}>{DOCUMENT_LABEL[v]}</option>)}
                  </select></div>
              </div>
              {minor && (
                <div className="ui-field"><label htmlFor="idGuardian">함께 온 법정대리인{f.document === 'FAMILY_RELATION_CERT' ? <> <span className="required">*</span></> : ''}</label>
                  <select id="idGuardian" className="ui-select" value={f.guardian} onChange={e => set('guardian', e.target.value as GuardianRelation | '')}>
                    <option value="">{f.document === 'FAMILY_RELATION_CERT' ? '골라 주세요' : '본인만 왔어요'}</option>
                    {Object.entries(GUARDIAN_LABEL).map(([v, l]) => <option key={v} value={v}>{l}</option>)}
                  </select></div>
              )}
              <div className="ui-field"><label htmlFor="idNote">메모{canCorrect ? ' (바로잡을 때는 사유 필수)' : ''}</label>
                <textarea id="idNote" className="ui-textarea" maxLength={500} rows={2} value={f.note} onChange={e => set('note', e.target.value)} placeholder="예: 대리인 없이 본인 방문" /></div>
              <label className="adm-id-seen">
                <input type="checkbox" checked={f.seen} onChange={e => set('seen', e.target.checked)} />
                <span>{f.document === 'FAMILY_RELATION_CERT'
                  ? '법정대리인 신분증 원본의 사진과 얼굴을 대조했고, 가족관계증명서(발급 3개월 이내)로 본인과의 관계를 확인했어요.'
                  : '신분증 원본의 사진과 본인 얼굴을 대조했고, 유효한 신분증이에요.'}</span>
              </label>
              <div className="adm-id-actions">
                <button type="button" className={`ui-btn is-primary${busy ? ' is-busy' : ''}`} disabled={busy} onClick={() => void save(false)}>인증 저장</button>
                {canCorrect && <button type="button" className="ui-btn is-warn" disabled={busy} onClick={() => void save(true)}>잘못 입력한 것 바로잡기</button>}
              </div>
            </div>
          )}

          {account.checks.length > 0 && (
            <div className="ui-card">
              <h2 className="ui-group-title">직원 확인 기록</h2>
              <ul className="ui-list">
                {account.checks.map(c => (
                  <li key={c.id} className="ui-row">
                    <div>
                      <div className="ui-row-title">{CHANNEL_LABEL[c.channel] ?? c.channel} · {DOCUMENT_LABEL[c.document_type] ?? c.document_type}{c.guardian_relation ? ` · 법정대리인(${GUARDIAN_LABEL[c.guardian_relation] ?? c.guardian_relation}) 동행` : ''}{c.correction ? ' · 바로잡음' : ''}</div>
                      <div className="ui-row-meta">{when(c.created_at)} · 담당 {c.staff_user_id === me.user_id ? '나' : c.staff_user_id.slice(0, 8)}{c.note ? ` · ${c.note}` : ''}</div>
                    </div>
                  </li>
                ))}
              </ul>
            </div>
          )}
        </>
      )}
    </div>
  );
}
