// 계정 정보 — 본인 확인·아티스트·수익 받기 설정. 토스 설정 화면처럼 '줄 + ›' 목록. 줄을 누르면 그 항목 하나만 고치는 창이 열린다.
import { useState } from 'react';
import { useToast } from '../components/Toast';
import { useConfirm } from '../components/Confirm';
import { Modal, useModalClose } from '../components/Modal';
import { Glyph } from '../components/Glyph';
import { PaymentSetupModal } from '../components/PaymentSetupModal';
import { setProfile, useProfile, type ProfileInfo } from '../store/profile';
import { isPaymentRegistered, usePayment } from '../store/payment';
import { patchAccount, taxCountryLabel, useAccount, VERIFY_LABEL, TAX_COUNTRIES, type TaxResidency } from '../store/account';
import { IdentityVerify } from '../components/IdentityVerify';
import { useAuth } from '../api/auth';
import { MOCK } from '../api/client';
import * as portal from '../api/portal';
import { errorMessage } from '../api/errors';
import { clearAll } from '../lib/storage';
import { useNavigate } from '../lib/router';

const COUNTRY_LABEL: Record<string, string> = {
  KR: '대한민국', US: '미국', JP: '일본', OTHER: '기타',
};

type Field = 'name' | 'email' | 'bio' | 'country';
const FIELD_TITLE: Record<Field, string> = {
  name: '활동명', email: '연락 이메일', bio: '아티스트 소개', country: '활동 국가',
};
const FIELD_ID: Record<Field, string> = {
  name: 'profileName', email: 'profileEmail', bio: 'profileBio', country: 'profileCountry',
};

/** 설정 목록의 한 줄 — 이름 · 오른쪽 값 · › */
function SetRow({ id, label, value, empty, onClick, danger }: {
  id?: string; label: string; value?: string; empty?: string; onClick: () => void; danger?: boolean;
}) {
  return (
    <button type="button" id={id} className={`aq-set-row${danger ? ' is-danger' : ''}`} onClick={onClick}>
      <span className="aq-set-label">{label}</span>
      {value ? <span className="aq-set-value">{value}</span> : empty ? <span className="aq-set-value is-empty">{empty}</span> : null}
      {!danger && <Glyph name="chevron-right" size={16} className="aq-set-chev" />}
    </button>
  );
}

/** 한 항목만 고치는 창 */
function FieldEditor({ field, profile, onSaved }: { field: Field; profile: ProfileInfo; onSaved: (next: ProfileInfo) => Promise<boolean> }) {
  const close = useModalClose();
  const [value, setValue] = useState(String(profile[field] ?? ''));
  const [error, setError] = useState('');
  const [saving, setSaving] = useState(false);
  const id = FIELD_ID[field];
  const changed = value.trim() !== String(profile[field] ?? '').trim();

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    const v = field === 'country' ? value : value.trim();
    if (field === 'name' && !v) { setError('활동명을 입력해 주세요.'); return; }
    if (field === 'email' && v && !/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(v)) { setError('이메일 형식을 확인해 주세요.'); return; }
    setSaving(true);
    const ok = await onSaved({ ...profile, [field]: v });
    setSaving(false);
    if (ok) close();
  };

  return (
    <form id="profileForm" className="aq-set-editor" onSubmit={submit} noValidate>
      <div className="ui-field">
        <label htmlFor={id} className="sr-only">{FIELD_TITLE[field]}</label>
        {field === 'bio' ? (
          <>
            <textarea id={id} rows={5} maxLength={1500} autoFocus placeholder="아티스트를 소개해 주세요." value={value} onChange={e => setValue(e.target.value)} />
            <p className="ui-field-help aq-counter">{value.length} / 1500</p>
          </>
        ) : field === 'country' ? (
          <select id={id} value={value || 'KR'} onChange={e => setValue(e.target.value)}>
            <option value="KR">대한민국</option>
            <option value="US">미국</option>
            <option value="JP">일본</option>
            <option value="OTHER">기타</option>
          </select>
        ) : (
          <input
            id={id} autoFocus
            type={field === 'email' ? 'email' : 'text'}
            maxLength={field === 'email' ? 180 : 120}
            autoComplete={field === 'email' ? 'email' : 'nickname'}
            placeholder={field === 'email' ? 'hello@example.com' : '아티스트명'}
            aria-invalid={!!error}
            value={value} onChange={e => { setValue(e.target.value); setError(''); }}
          />
        )}
        {error && <p className="ui-field-help aq-help-error">{error}</p>}
      </div>
      <button type="submit" className={`ui-btn is-primary is-block${saving ? ' is-busy' : ''}`} disabled={saving || !changed}>
        {saving ? '저장하는 중' : '저장'}
      </button>
    </form>
  );
}

/** 해외 납세 의무 — 대한민국 외 국가에 세금을 낼 의무가 있는지(정산 원천징수 확인용) */
function TaxEditor({ current, onSaved }: { current: TaxResidency | null; onSaved: (t: TaxResidency) => void }) {
  const close = useModalClose();
  const [t, setT] = useState<TaxResidency>(current ?? { foreign: false, country: 'US', tin: '', usPerson: false });
  const [error, setError] = useState('');
  const set = (p: Partial<TaxResidency>) => { setT(prev => ({ ...prev, ...p })); setError(''); };

  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (t.foreign && !t.country) { setError('납세 의무가 있는 국가를 골라 주세요.'); return; }
    onSaved(t.foreign ? { ...t, tin: t.tin.trim(), usPerson: t.country === 'US' && t.usPerson } : { foreign: false, country: '', tin: '', usPerson: false });
    close();
  };

  return (
    <form className="aq-set-editor aq-tax" onSubmit={submit} noValidate>
      <p className="aq-tax-lead">정산금을 지급할 때 적용할 세율을 확인하는 데 쓰여요.</p>
      <div className="aq-choice-row aq-tax-choice" role="radiogroup" aria-label="해외 납세 의무">
        {([[false, '대한민국에만 있어요', '국내 거주자로 정산해요.'], [true, '다른 나라에도 있어요', '해당 국가 납세 정보가 필요해요.']] as const).map(([v, title, sub]) => (
          <label key={title} className={`aq-choice${t.foreign === v ? ' is-on' : ''}`}>
            <input type="radio" name="taxForeign" checked={t.foreign === v} onChange={() => set({ foreign: v })} />
            <strong>{title}</strong>
            <small>{sub}</small>
          </label>
        ))}
      </div>
      {t.foreign && (
        <div className="aq-tax-more aq-reveal">
          <div className="ui-field">
            <label htmlFor="taxCountry">납세 의무가 있는 국가</label>
            <select id="taxCountry" value={t.country} onChange={e => set({ country: e.target.value })}>
              {TAX_COUNTRIES.map(c => <option key={c.code} value={c.code}>{c.label}</option>)}
            </select>
          </div>
          <div className="ui-field">
            <label htmlFor="taxTin">납세자번호(TIN) <span className="aq-optional">선택</span></label>
            <input id="taxTin" maxLength={30} autoComplete="off" placeholder={t.country === 'US' ? 'SSN 또는 ITIN' : '해당 국가 납세자번호'} value={t.tin} onChange={e => set({ tin: e.target.value })} />
          </div>
          {t.country === 'US' && (
            <label className="ui-check aq-tax-us">
              <input type="checkbox" checked={t.usPerson} onChange={e => set({ usPerson: e.target.checked })} />
              <span>미국 시민권자 또는 영주권자예요</span>
            </label>
          )}
        </div>
      )}
      {error && <p className="ui-field-help aq-help-error" role="alert">{error}</p>}
      <button type="submit" className="ui-btn is-primary is-block">저장</button>
    </form>
  );
}

export function Profile() {
  const toast = useToast();
  const confirm = useConfirm();
  const nav = useNavigate();
  const { logout } = useAuth();
  const profile = useProfile();
  const payment = usePayment();
  const account = useAccount();
  const [editing, setEditing] = useState<Field | null>(null);
  const [accountEdit, setAccountEdit] = useState<'verify' | 'tax' | null>(null);
  const [showPayment, setShowPayment] = useState(false);

  const save = async (next: ProfileInfo): Promise<boolean> => {
    if (!MOCK) {
      try {
        await portal.saveProfile(next);
      } catch (err) {
        toast(errorMessage(err, '프로필을 저장하지 못했어요. 잠시 후 다시 시도해 주세요.'));
        return false;
      }
    }
    setProfile(next);
    toast('저장했어요.');
    return true;
  };

  const doLogout = async () => {
    const ok = await confirm({ title: '로그아웃할까요?', message: '작성 중인 발매는 임시 저장돼 있어요.', confirmLabel: '로그아웃' });
    if (!ok) return;
    await logout();
    toast('로그아웃했어요.', 'info');
    nav('/login', { replace: true });
  };

  const resetData = async () => {
    const ok = await confirm({
      title: '저장된 데이터를 초기화할까요?',
      message: '이 브라우저에 저장된 발매·서류·정산 기록이 모두 지워지고 처음 상태로 돌아가요.',
      confirmLabel: '초기화', danger: true,
    });
    if (!ok) return;
    clearAll();
    window.location.reload();
  };

  const registered = isPaymentRegistered(payment);
  const country = COUNTRY_LABEL[profile.country] || '대한민국';
  const bioLine = profile.bio.trim().split('\n')[0];

  return (
    <div id="view-profile" className="view aq-settings">
      <div className="ui-page-head">
        <div>
          <h1>계정 정보</h1>
        </div>
      </div>

      <button type="button" className="aq-set-profile" onClick={() => setEditing('name')}>
        <span className="ui-avatar is-xl aq-profile-avatar" aria-hidden="true">{(profile.name || 'A').slice(0, 1)}</span>
        <span className="min-0">
          <strong>{profile.name || '활동명을 입력해 주세요'}</strong>
          <small>{profile.email || '연락 이메일 미입력'} · {country}</small>
        </span>
        <Glyph name="chevron-right" size={18} className="aq-set-chev" />
      </button>

      <section className="aq-set-card" aria-labelledby="setIdentity">
        <h2 id="setIdentity">본인 확인</h2>
        <SetRow id="accountRowName" label="실명" value={account.realName} empty="본인인증 후 등록돼요" onClick={() => setAccountEdit('verify')} />
        <SetRow
          id="accountRowVerify" label="본인인증"
          value={account.verified ? `${VERIFY_LABEL[account.verified.method]} 인증 완료` : undefined}
          empty="인증하기" onClick={() => setAccountEdit('verify')}
        />
      </section>

      <section className="aq-set-card" aria-labelledby="setArtist">
        <h2 id="setArtist">아티스트</h2>
        <SetRow id="profileRowName" label="활동명" value={profile.name} empty="입력하기" onClick={() => setEditing('name')} />
        <SetRow id="profileRowEmail" label="연락 이메일" value={profile.email} empty="입력하기" onClick={() => setEditing('email')} />
        <SetRow id="profileRowBio" label="아티스트 소개" value={bioLine} empty="입력하기" onClick={() => setEditing('bio')} />
        <SetRow id="profileRowCountry" label="활동 국가" value={country} onClick={() => setEditing('country')} />
      </section>

      <section className="aq-set-card" aria-labelledby="setPayout">
        <h2 id="setPayout">수익 받기</h2>
        <SetRow
          id="profileRowPayout" label="정산 계좌"
          value={registered ? `${payment.bank} ···${payment.last4}` : undefined} empty="등록하기"
          onClick={() => setShowPayment(true)}
        />
        <SetRow
          id="accountRowTax" label="해외 납세 의무"
          value={account.tax ? (account.tax.foreign ? `있음 · ${taxCountryLabel(account.tax.country)}` : '없음') : undefined}
          empty="설정하기" onClick={() => setAccountEdit('tax')}
        />
        <SetRow label="정산·지급 내역" onClick={() => nav('/settlement')} />
      </section>

      <section className="aq-set-card" aria-labelledby="setLegal">
        <h2 id="setLegal">약관</h2>
        <SetRow label="서비스 이용약관" onClick={() => nav('/terms')} />
        <SetRow label="정산금 지급 및 계좌 확인 약관" onClick={() => nav('/terms/settlement')} />
      </section>

      <section className="aq-set-card">
        <SetRow label="로그아웃" danger onClick={() => void doLogout()} />
      </section>

      {MOCK && (
        <p className="aq-reset-line">
          이 브라우저에만 저장되는 체험 모드예요.{' '}
          <button type="button" className="ui-btn is-soft is-sm" onClick={resetData}>저장된 데이터 초기화</button>
        </p>
      )}

      {editing && (
        <Modal title={FIELD_TITLE[editing]} onClose={() => setEditing(null)} modalClass="aq-set-modal">
          <FieldEditor field={editing} profile={profile} onSaved={save} />
        </Modal>
      )}
      {showPayment && <PaymentSetupModal onClose={() => setShowPayment(false)} />}
      {accountEdit === 'verify' && (
        <Modal title={account.verified ? '본인인증 다시 하기' : '본인인증'} onClose={() => setAccountEdit(null)} modalClass="aq-set-modal">
          <IdentityVerify onDone={() => toast('본인인증을 마쳤어요.')} />
        </Modal>
      )}
      {accountEdit === 'tax' && (
        <Modal title="해외 납세 의무" onClose={() => setAccountEdit(null)} modalClass="aq-set-modal">
          <TaxEditor current={account.tax} onSaved={t => { patchAccount({ tax: t }); toast('저장했어요.'); }} />
        </Modal>
      )}
    </div>
  );
}
