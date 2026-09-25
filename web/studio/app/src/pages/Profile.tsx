import { useState } from 'react';

export function Profile() {
  const [name, setName] = useState('서린');
  const [email, setEmail] = useState('artist.demo@example.com');
  const [bio, setBio] = useState('도시의 풍경과 하루의 감정을 음악으로 기록합니다.');
  const [country, setCountry] = useState('KR');
  const [bank, setBank] = useState('');
  const [account, setAccount] = useState('');
  const [saved, setSaved] = useState(false);

  const saveProfile = (e: React.FormEvent) => {
    e.preventDefault();
    setSaved(true);
  };

  const savePayment = () => {
    setSaved(true);
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

      {saved && (
        <div className="notice success" style={{ marginBottom: 20 }}>
          저장됐어요. (테스트 모드)
        </div>
      )}

      <div className="profile-columns">
        <div>
          <h2 className="subhead">아티스트 프로필</h2>
          <form onSubmit={saveProfile}>
            <div className="profile-avatar" aria-hidden="true">{name.charAt(0) || 'A'}</div>
            <div className="field">
              <label htmlFor="profileName">활동명 <span className="required">*</span></label>
              <input
                id="profileName" maxLength={120} required autoComplete="nickname"
                placeholder="아티스트명" value={name} onChange={e => setName(e.target.value)}
              />
            </div>
            <div className="field">
              <label htmlFor="profileEmail">연락 이메일</label>
              <input
                id="profileEmail" type="email" maxLength={180} autoComplete="email"
                placeholder="hello@example.com" value={email} onChange={e => setEmail(e.target.value)}
              />
            </div>
            <div className="field">
              <label htmlFor="profileBio">아티스트 소개</label>
              <textarea
                id="profileBio" rows={4} maxLength={1500}
                placeholder="아티스트를 소개해 주세요."
                value={bio} onChange={e => setBio(e.target.value)}
              />
            </div>
            <div className="field">
              <label htmlFor="profileCountry">활동 국가</label>
              <select id="profileCountry" value={country} onChange={e => setCountry(e.target.value)}>
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
          <section className="aq-payment-summary" aria-live="polite">
            <div>
              <strong>{bank && account ? `${bank} · ${account}` : '수령 정보를 등록해 주세요.'}</strong>
              <p>정산된 수익을 받을 계좌를 등록해 주세요. (테스트 모드)</p>
            </div>
          </section>
          <div className="field">
            <label htmlFor="payBank">은행</label>
            <input
              id="payBank" placeholder="예: 토스뱅크"
              value={bank} onChange={e => setBank(e.target.value)}
            />
          </div>
          <div className="field">
            <label htmlFor="payAccount">계좌번호</label>
            <input
              id="payAccount" inputMode="numeric" placeholder="'-' 없이 입력"
              value={account} onChange={e => setAccount(e.target.value)}
            />
          </div>
          <button type="button" className="button" onClick={savePayment}>수령 정보 등록</button>
          <div className="demo-settings">
            <h3>작업 정보 초기화</h3>
            <p>현재 시안의 발매·정산·서류 자료를 처음 상태로 되돌려요.</p>
            <button type="button" className="button danger" onClick={() => setSaved(false)}>
              기본 데이터로 초기화
            </button>
          </div>
        </div>
      </div>
    </>
  );
}
