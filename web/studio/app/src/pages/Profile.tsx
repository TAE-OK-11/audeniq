import { useState } from 'react';
import { Modal } from '../components/Modal';
import { useToast } from '../components/Toast';

const FINANCIAL_INSTITUTIONS: Record<string, { label: string; note: string; items: [string, string][] }> = {
  bank: {
    label: '은행', note: '은행·인터넷은행·특수은행·상호금융',
    items: [['IBK기업은행','IBK'],['신한은행','신한'],['KB국민은행','KB'],['하나은행','하나'],['우리은행','우리'],['NH농협은행','NH'],['SC제일은행','SC'],['KDB산업은행','KDB'],['카카오뱅크','B'],['부산은행','BNK'],['iM뱅크','iM'],['경남은행','BNK'],['토스뱅크','T'],['케이뱅크','K'],['수협은행','Sh'],['한국씨티은행','citi'],['새마을금고','MG'],['신협','CU'],['우체국','POST'],['산림조합','SJ'],['전북은행','JB'],['광주은행','KJ'],['제주은행','제주']],
  },
  savings: {
    label: '저축은행', note: '주요 저축은행을 자산 규모와 이용 빈도 순으로 정리했어요.',
    items: [['SBI저축은행','SBI'],['OK저축은행','OK'],['한국투자저축은행','한국'],['웰컴저축은행','W'],['애큐온저축은행','ACUON'],['페퍼저축은행','PEPPER'],['신한저축은행','신한'],['KB저축은행','KB'],['하나저축은행','하나'],['우리금융저축은행','우리'],['NH저축은행','NH'],['IBK저축은행','IBK'],['BNK저축은행','BNK'],['다올저축은행','다올'],['키움YES저축은행','YES'],['JT친애저축은행','JT'],['JT저축은행','JT'],['상상인저축은행','상상인'],['상상인플러스저축은행','상상인+'],['대신저축은행','대신'],['DB저축은행','DB'],['OSB저축은행','OSB'],['모아저축은행','MOA'],['푸른저축은행','푸른'],['스마트저축은행','SMART'],['예가람저축은행','예가람'],['바로저축은행','바로'],['유안타저축은행','유안타'],['동원제일저축은행','동원'],['청주저축은행','청주']],
  },
  securities: {
    label: '증권사', note: '주요 증권사를 자기자본·고객 이용 규모를 고려해 정리했어요.',
    items: [['미래에셋증권','미래'],['한국투자증권','한국'],['NH투자증권','NH'],['삼성증권','삼성'],['KB증권','KB'],['신한투자증권','신한'],['키움증권','키움'],['하나증권','하나'],['메리츠증권','meritz'],['대신증권','대신'],['토스증권','T'],['카카오페이증권','K'],['우리투자증권','우리'],['한화투자증권','한화'],['유안타증권','유안타'],['현대차증권','HMC'],['교보증권','교보'],['DB증권','DB'],['iM증권','iM'],['LS증권','LS'],['SK증권','SK'],['신영증권','신영'],['유진투자증권','유진'],['IBK투자증권','IBK'],['BNK투자증권','BNK'],['다올투자증권','다올'],['부국증권','부국'],['흥국증권','흥국'],['케이프투자증권','CAPE'],['상상인증권','상상인']],
  },
};

const PAY_TYPES = [
  { value: 'personal', label: '개인', desc: '본인 명의 계좌로 받아요.' },
  { value: 'business', label: '개인사업자', desc: '사업자 명의 또는 대표자 계좌로 받아요.' },
  { value: 'corporate', label: '법인', desc: '법인 명의 계좌로 받아요.' },
];

const TYPE_LABEL: Record<string, string> = { personal: '개인', business: '개인사업자', corporate: '법인' };

interface PaymentInfo {
  recipient: string; type: string; bank: string;
  accountNumber: string; last4: string; registeredAt: string;
}

function PaymentWizard({ onDone, onClose }: { onDone: (p: PaymentInfo) => void; onClose: () => void }) {
  const toast = useToast();
  const [step, setStep] = useState(0);
  const [recipient, setRecipient] = useState('');
  const [type, setType] = useState('personal');
  const [category, setCategory] = useState('bank');
  const [bankSearch, setBankSearch] = useState('');
  const [bank, setBank] = useState('');
  const [account, setAccount] = useState('');
  const [owner, setOwner] = useState('');
  const [agreeAll, setAgreeAll] = useState(false);
  const [agrees, setAgrees] = useState([false, false, false, false]);

  const cat = FINANCIAL_INSTITUTIONS[category];
  const filteredBanks = cat.items.filter(([name]) =>
    name.toLowerCase().includes(bankSearch.toLowerCase()));

  const allAgreed = agrees.every(Boolean);
  const syncAgree = (arr: boolean[]) => {
    setAgrees(arr);
    setAgreeAll(arr.every(Boolean));
  };

  const nextFrom0 = () => {
    if (!recipient.trim()) { toast('수령인 이름 또는 상호를 입력해 주세요.'); return; }
    setStep(1);
  };

  const nextFrom1 = () => {
    if (!bank) { toast('금융기관을 선택해 주세요.'); return; }
    setStep(2);
  };

  const nextFrom2 = () => {
    const acct = account.replace(/\D/g, '');
    if (acct.length < 7) { toast('계좌번호를 다시 확인해 주세요.'); return; }
    if (!owner.trim()) { toast('예금주를 입력해 주세요.'); return; }
    setStep(3);
  };

  const submit = () => {
    if (!allAgreed) { toast('필수 약관을 모두 확인해 주세요.'); return; }
    const acct = account.replace(/\D/g, '');
    onDone({
      recipient: owner.trim() || recipient.trim(),
      type, bank,
      accountNumber: acct,
      last4: acct.slice(-4),
      registeredAt: new Date().toISOString().slice(0, 10),
    });
    setStep(4);
  };

  return (
    <div>
      {step === 0 && (
        <>
          <p className="eyebrow">PAYOUT PROFILE</p>
          <h1>수익을 받을<br />정보를 알려주세요.</h1>
          <p className="aq-pay-lead">계약과 정산 서류에 표시할 수령인 정보를 입력해 주세요.</p>
          <div className="field aq-pay-field">
            <label htmlFor="aqPayRecipient">수령인 이름 또는 상호</label>
            <input
              id="aqPayRecipient" maxLength={120} autoComplete="name"
              value={recipient} onChange={e => setRecipient(e.target.value)}
              placeholder="예금주 또는 법인명"
            />
          </div>
          <div className="aq-pay-type-list" role="radiogroup" aria-label="수령인 유형">
            {PAY_TYPES.map(t => (
              <label key={t.value} className="aq-pay-type">
                <input
                  type="radio" name="aqPayType" value={t.value}
                  checked={type === t.value} onChange={() => setType(t.value)}
                />
                <span><strong>{t.label}</strong><small>{t.desc}</small></span>
              </label>
            ))}
          </div>
          <button type="button" className="button studio-submit-wide" onClick={nextFrom0} style={{ marginTop: 20 }}>
            다음으로
          </button>
        </>
      )}

      {step === 1 && (
        <>
          <p className="eyebrow">PAYOUT ACCOUNT</p>
          <h1>수익을 받을<br />금융기관을 선택해 주세요.</h1>
          <p className="aq-pay-lead">은행, 저축은행, 증권사 계좌를 등록할 수 있어요.</p>
          <div className="aq-bank-tabs" role="tablist" aria-label="금융기관 종류">
            {Object.entries(FINANCIAL_INSTITUTIONS).map(([key, value]) => (
              <button
                key={key} type="button" role="tab"
                aria-selected={key === category}
                onClick={() => { setCategory(key); setBankSearch(''); }}
              >
                {value.label}
              </button>
            ))}
          </div>
          <p className="aq-bank-category-note">{cat.note}</p>
          <div className="aq-bank-search">
            <input
              type="search" placeholder="금융기관 검색"
              value={bankSearch} onChange={e => setBankSearch(e.target.value)}
            />
          </div>
          <section className="aq-bank-group">
            <h2>{cat.label}</h2>
            <div className="aq-bank-grid">
              {filteredBanks.map(([name, mark]) => (
                <button
                  key={name} type="button"
                  className={`aq-bank ${bank === name ? 'selected' : ''}`}
                  onClick={() => { setBank(name); setStep(2); }}
                >
                  <span className="aq-bank-logo" aria-hidden="true">{mark}</span>
                  <strong>{name}</strong>
                </button>
              ))}
            </div>
          </section>
          <div style={{ display: 'flex', gap: 8, marginTop: 20 }}>
            <button type="button" className="button secondary" onClick={() => setStep(0)}>이전</button>
            <button type="button" className="button" onClick={nextFrom1} style={{ flex: 1 }}>다음으로</button>
          </div>
        </>
      )}

      {step === 2 && (
        <>
          <p className="eyebrow">ACCOUNT NUMBER</p>
          <h1>{bank}<br />계좌번호를 입력해 주세요.</h1>
          <div className="aq-selected-bank">
            <span className="aq-bank-logo" aria-hidden="true">{FINANCIAL_INSTITUTIONS[category].items.find(x => x[0] === bank)?.[1] || ''}</span>
            <div><strong>{bank}</strong><small>수익 정산 계좌</small></div>
          </div>
          <div className="field aq-pay-field">
            <label htmlFor="aqPayAccount">계좌번호</label>
            <input
              id="aqPayAccount" inputMode="numeric" autoComplete="off" maxLength={20}
              value={account} onChange={e => setAccount(e.target.value.replace(/\D/g, ''))}
              placeholder="숫자만 입력"
            />
          </div>
          <div className="field aq-pay-field">
            <label htmlFor="aqPayOwner">예금주</label>
            <input
              id="aqPayOwner" maxLength={120}
              value={owner} onChange={e => setOwner(e.target.value)}
              placeholder="예금주명"
            />
          </div>
          <p className="aq-pay-privacy">
            계좌번호 전체값을 등록하고 화면에는 마스킹해 표시해요. 실제 서비스에서는 암호화된 서버 저장소에 안전하게 보관돼요.
          </p>
          <div style={{ display: 'flex', gap: 8, marginTop: 20 }}>
            <button type="button" className="button secondary" onClick={() => setStep(1)}>이전</button>
            <button type="button" className="button" onClick={nextFrom2} style={{ flex: 1 }}>계좌 확인</button>
          </div>
        </>
      )}

      {step === 3 && (
        <>
          <p className="eyebrow">AGREEMENT</p>
          <h1>수령 계좌 등록에<br />동의해 주세요.</h1>
          <p className="aq-pay-lead">정산금 지급과 계좌 확인에 필요한 항목이에요.</p>
          <label className="aq-agree-all">
            <input
              type="checkbox" checked={agreeAll}
              onChange={e => { const v = e.target.checked; syncAgree([v, v, v, v]); }}
            />
            <span><strong>필수 약관 전체 동의</strong><small>아래 항목을 모두 확인했어요.</small></span>
          </label>
          <div className="aq-agree-list">
            {['정산금 지급 및 계좌 확인 약관', '개인정보 수집·이용 동의', '금융정보 처리 동의', '수령인 정보 정확성 확인'].map((label, i) => (
              <label key={label}>
                <input
                  type="checkbox" checked={agrees[i]}
                  onChange={e => { const arr = [...agrees]; arr[i] = e.target.checked; syncAgree(arr); }}
                />
                <span>{label}</span>
                <button type="button" aria-label="약관 보기" onClick={() => toast('약관 내용을 준비하고 있어요.')}>›</button>
              </label>
            ))}
          </div>
          <div className="aq-pay-review">
            <span className="aq-bank-logo" aria-hidden="true">{FINANCIAL_INSTITUTIONS[category].items.find(x => x[0] === bank)?.[1] || ''}</span>
            <div>
              <strong>{bank} · •••• {account.replace(/\D/g, '').slice(-4)}</strong>
              <small>{owner || recipient} · {TYPE_LABEL[type]}</small>
            </div>
          </div>
          <div style={{ display: 'flex', gap: 8, marginTop: 20 }}>
            <button type="button" className="button secondary" onClick={() => setStep(2)}>이전</button>
            <button type="button" className="button" onClick={submit} style={{ flex: 1 }} disabled={!allAgreed}>
              동의하고 등록
            </button>
          </div>
        </>
      )}

      {step === 4 && (
        <div className="aq-pay-success">
          <div className="aq-success-check">✓</div>
          <p className="eyebrow">PAYOUT REGISTERED</p>
          <h1>수령 정보가<br />등록됐어요.</h1>
          <div className="aq-pay-review">
            <span className="aq-bank-logo" aria-hidden="true">{FINANCIAL_INSTITUTIONS[category].items.find(x => x[0] === bank)?.[1] || ''}</span>
            <div>
              <strong>{bank} · •••• {account.replace(/\D/g, '').slice(-4)}</strong>
              <small>{owner || recipient} · {TYPE_LABEL[type]}</small>
            </div>
          </div>
          <button type="button" className="button studio-submit-wide" onClick={onClose} style={{ marginTop: 20 }}>
            완료
          </button>
        </div>
      )}
    </div>
  );
}

export function Profile() {
  const toast = useToast();
  const [name, setName] = useState('서린');
  const [email, setEmail] = useState('artist.demo@example.com');
  const [bio, setBio] = useState('도시의 풍경과 하루의 감정을 음악으로 기록합니다.');
  const [country, setCountry] = useState('KR');
  const [payment, setPayment] = useState<PaymentInfo | null>(null);
  const [showWizard, setShowWizard] = useState(false);

  const saveProfile = (e: React.FormEvent) => {
    e.preventDefault();
    toast('프로필이 저장됐어요.');
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
            {payment ? (
              <div>
                <strong>{payment.bank} · •••• {payment.last4}</strong>
                <p>{payment.recipient} · {TYPE_LABEL[payment.type]} · 등록일 {payment.registeredAt}</p>
              </div>
            ) : (
              <div>
                <strong>수령 정보를 등록해 주세요.</strong>
                <p>정산된 수익을 받을 계좌를 등록해 주세요. (테스트 모드)</p>
              </div>
            )}
            <button type="button" className="button secondary" onClick={() => setShowWizard(true)}>
              {payment ? '수령 정보 변경' : '수령 정보 등록'}
            </button>
          </section>
        </div>
      </div>

      {showWizard && (
        <Modal title="수익 정산 정보 등록" onClose={() => setShowWizard(false)}>
          <PaymentWizard
            onDone={p => { setPayment(p); toast('수익 정산 정보가 등록됐어요.'); }}
            onClose={() => setShowWizard(false)}
          />
        </Modal>
      )}
    </>
  );
}
