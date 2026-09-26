// 아티스트 정보 — 라이브 view-profile 대응 (프로필 + 수령 정보)
import { useEffect, useState } from 'react';
import { useToast } from '../components/Toast';
import { useConfirm } from '../components/Confirm';
import { BankLogo } from '../components/BankLogo';
import { PaymentSetupModal } from '../components/PaymentSetupModal';
import { setProfile, useProfile, type ProfileInfo } from '../store/profile';
import { isPaymentRegistered, TYPE_LABEL, usePayment } from '../store/payment';
import { MOCK } from '../api/client';
import * as portal from '../api/portal';
import { errorMessage } from '../api/errors';
import { clearAll } from '../lib/storage';
import { localStamp } from '../lib/format';

const COUNTRY_LABEL: Record<string, string> = {
  KR: '대한민국', US: '미국', JP: '일본', OTHER: '기타',
};

export function Profile() {
  const toast = useToast();
  const confirm = useConfirm();
  const profile = useProfile();
  const payment = usePayment();
  const [draft, setDraft] = useState<ProfileInfo>(profile);
  const [saving, setSaving] = useState(false);
  // 실서버: 로그인 직후 서버에서 불러온 프로필을 입력칸에 반영
  useEffect(() => { setDraft(profile); }, [profile]);
  const [showPayment, setShowPayment] = useState(false);
  const [emailError, setEmailError] = useState('');

  const dirty = JSON.stringify(draft) !== JSON.stringify(profile);
  const setField = <K extends keyof ProfileInfo>(k: K, v: ProfileInfo[K]) => setDraft(d => ({ ...d, [k]: v }));

  const saveProfile = async (e: React.FormEvent) => {
    e.preventDefault();
    const next = { ...draft, name: draft.name.trim(), email: draft.email.trim(), bio: draft.bio.trim() };
    if (!next.name) { toast('활동명을 입력해 주세요.'); return; }
    if (next.email && !/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(next.email)) {
      setEmailError('이메일 형식을 확인해 주세요.');
      document.getElementById('profileEmail')?.focus();
      return;
    }
    setEmailError('');
    if (!MOCK) {
      setSaving(true);
      try {
        await portal.saveProfile(next);
      } catch (err) {
        toast(errorMessage(err, '프로필을 저장하지 못했어요. 잠시 후 다시 시도해 주세요.'));
        return;
      } finally {
        setSaving(false);
      }
    }
    setProfile(next);
    setDraft(next);
    toast('프로필을 저장했어요.');
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

  return (
    <div id="view-profile" className="view">
      <div className="view-title">
        <div>
          <p className="eyebrow">ARTIST</p>
          <h1>아티스트 정보</h1>
          <p>아티스트 프로필과 수익을 받을 정보를 관리해 보세요.</p>
        </div>
      </div>

      <div className="aq-profile-card">
        <div className="aq-profile-head">
          <div className="profile-avatar aq-profile-avatar" aria-hidden="true">
            {(draft.name || 'A').slice(0, 1)}
          </div>
          <div className="min-0">
            <strong>{draft.name || '아티스트명 미입력'}</strong>
            <p>{draft.email || '연락 이메일 미입력'} · {COUNTRY_LABEL[draft.country] || '대한민국'}</p>
          </div>
        </div>

        <form id="profileForm" onSubmit={saveProfile} noValidate>
          <div className="field">
            <label htmlFor="profileName">활동명 <span className="required">*</span></label>
            <input
              id="profileName" maxLength={120} required autoComplete="nickname"
              placeholder="아티스트명" value={draft.name} onChange={e => setField('name', e.target.value)}
            />
          </div>
          <div className="field">
            <label htmlFor="profileEmail">연락 이메일</label>
            <input
              id="profileEmail" type="email" maxLength={180} autoComplete="email" aria-invalid={!!emailError}
              placeholder="hello@example.com" value={draft.email}
              onChange={e => { setField('email', e.target.value); setEmailError(''); }}
            />
            {emailError && <p className="help aq-help-error">{emailError}</p>}
          </div>
          <div className="field">
            <label htmlFor="profileBio">아티스트 소개</label>
            <textarea
              id="profileBio" rows={4} maxLength={1500}
              placeholder="아티스트를 소개해 주세요."
              value={draft.bio} onChange={e => setField('bio', e.target.value)}
            />
            <p className="help aq-counter">{draft.bio.length} / 1500</p>
          </div>
          <div className="field">
            <label htmlFor="profileCountry">활동 국가</label>
            <select id="profileCountry" value={draft.country} onChange={e => setField('country', e.target.value)}>
              <option value="KR">대한민국</option>
              <option value="US">미국</option>
              <option value="JP">일본</option>
              <option value="OTHER">기타</option>
            </select>
          </div>
          <div className="aq-form-actions">
            {dirty && (
              <button type="button" className="button secondary" onClick={() => setDraft(profile)}>되돌리기</button>
            )}
            <button type="submit" className={`button studio-submit-wide${saving ? ' is-busy' : ''}`} disabled={!dirty || saving}>
              {saving ? '저장하는 중' : dirty ? '프로필 저장' : '저장된 상태예요'}
            </button>
          </div>
        </form>
      </div>

      <section className="aq-profile-card aq-payment-card" aria-labelledby="payoutHead">
        <div className="section-top">
          <h2 id="payoutHead">수익을 받을 정보</h2>
          <button type="button" className="link-btn" onClick={() => setShowPayment(true)}>{registered ? '변경' : '등록하기'}</button>
        </div>
        {registered ? (
          <div className="settle-hero-account">
            <BankLogo name={payment.bank} />
            <span className="min-0">
              {payment.bank} · •••• {payment.last4} · {payment.recipient} ({TYPE_LABEL[payment.type]})
              <small className="aq-sub-line">{payment.registeredAt ? `${localStamp(payment.registeredAt)} 등록` : ''}</small>
            </span>
          </div>
        ) : (
          <p className="small muted">정산금을 받을 계좌를 등록하면 정산·지급 화면에서 바로 수익을 받을 수 있어요.</p>
        )}
      </section>

      {MOCK && (
        <p className="aq-reset-line">
          이 브라우저에만 저장되는 체험 모드예요.{' '}
          <button type="button" className="link-btn" onClick={resetData}>저장된 데이터 초기화</button>
        </p>
      )}

      {showPayment && <PaymentSetupModal onClose={() => setShowPayment(false)} />}
    </div>
  );
}
