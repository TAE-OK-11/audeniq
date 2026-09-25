// 아티스트·정산 정보 — 라이브 view-profile / renderProfile / openPaymentSetup 대응
import { useState } from 'react';
import { useToast } from '../components/Toast';
import { PaymentSetupModal } from '../components/PaymentSetupModal';
import { BankLogo } from '../components/BankLogo';
import { isPaymentRegistered, usePayment, TYPE_LABEL } from '../store/payment';
import { setProfile, useProfile } from '../store/profile';

export function Profile() {
  const toast = useToast();
  const profile = useProfile();
  const payment = usePayment();
  const [showWizard, setShowWizard] = useState(false);

  const registered = isPaymentRegistered(payment);

  const saveProfile = (e: React.FormEvent) => {
    e.preventDefault();
    const form = e.target as HTMLFormElement;
    const data = new FormData(form);
    setProfile({
      name: String(data.get('profileName') || '').trim(),
      email: String(data.get('profileEmail') || '').trim(),
      bio: String(data.get('profileBio') || '').trim(),
      country: String(data.get('profileCountry') || 'KR'),
    });
    toast('프로필을 저장했어요.');
  };

  return (
    <>
      <div className="view-title">
        <div>
          <p className="eyebrow">ARTIST &amp; SETTINGS</p>
          <h1>아티스트·정산 정보</h1>
          <p>아티스트 프로필과 수익을 받을 정보를 관리해 보세요.</p>
        </div>
      </div>

      <div className="profile-columns">
        <div>
          <h2 className="subhead">아티스트 프로필</h2>
          <form id="profileForm" onSubmit={saveProfile}>
            <div className="profile-avatar" id="profileAvatar">{(profile.name || 'A').slice(0, 1)}</div>
            <div className="field">
              <label htmlFor="profileName">활동명 <span className="required">*</span></label>
              <input
                id="profileName" name="profileName" maxLength={120} required autoComplete="nickname"
                placeholder="아티스트명" defaultValue={profile.name}
              />
            </div>
            <div className="field">
              <label htmlFor="profileEmail">연락 이메일</label>
              <input
                id="profileEmail" name="profileEmail" type="email" maxLength={180} autoComplete="email"
                placeholder="hello@example.com" defaultValue={profile.email}
              />
            </div>
            <div className="field">
              <label htmlFor="profileBio">아티스트 소개</label>
              <textarea
                id="profileBio" name="profileBio" rows={4} maxLength={1500}
                placeholder="아티스트를 소개해 주세요."
                defaultValue={profile.bio}
              />
            </div>
            <div className="field">
              <label htmlFor="profileCountry">활동 국가</label>
              <select id="profileCountry" name="profileCountry" defaultValue={profile.country}>
                <option value="KR">대한민국</option>
                <option value="US">미국</option>
                <option value="JP">일본</option>
                <option value="OTHER">기타</option>
              </select>
            </div>
            <button type="submit" className="button">프로필 저장</button>
          </form>
        </div>

        <div>
          <h2 className="subhead">수익 정산 정보</h2>
          <section className="aq-payment-summary" id="paymentSummary" aria-live="polite">
            {registered && payment ? (
              <>
                <BankLogo name={payment.bank} />
                <div>
                  <span className="aq-payment-state">등록 완료</span>
                  <strong>{payment.bank} · •••• {payment.last4}</strong>
                  <p>{payment.recipient} · {TYPE_LABEL[payment.type]}</p>
                </div>
              </>
            ) : (
              <>
                <div className="aq-payment-badge is-empty" aria-hidden="true">₩</div>
                <div>
                  <span className="aq-payment-state">등록 필요</span>
                  <strong>수익을 받을 정보를 등록해 주세요.</strong>
                  <p>수령인과 계좌 정보를 단계별로 안전하게 입력해요.</p>
                </div>
              </>
            )}
            <button
              type="button" className="button secondary" id="openPaymentSetup"
              onClick={() => setShowWizard(true)}
            >
              {registered ? '수령 정보 변경' : '수령 정보 등록'}
            </button>
          </section>
        </div>
      </div>

      {showWizard && <PaymentSetupModal onClose={() => setShowWizard(false)} />}
    </>
  );
}
