// 아티스트 정보 — 토스 설정 화면처럼 '줄 + ›' 목록. 줄을 누르면 그 항목 하나만 고치는 창이 열린다.
import { useState } from 'react';
import { useToast } from '../components/Toast';
import { useConfirm } from '../components/Confirm';
import { Modal, useModalClose } from '../components/Modal';
import { Glyph } from '../components/Glyph';
import { PaymentSetupModal } from '../components/PaymentSetupModal';
import { setProfile, useProfile, type ProfileInfo } from '../store/profile';
import { isPaymentRegistered, usePayment } from '../store/payment';
import { useUnreadCount } from '../store/support';
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

export function Profile() {
  const toast = useToast();
  const confirm = useConfirm();
  const nav = useNavigate();
  const { logout } = useAuth();
  const profile = useProfile();
  const payment = usePayment();
  const unread = useUnreadCount();
  const [editing, setEditing] = useState<Field | null>(null);
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
          <h1>아티스트 정보</h1>
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
        <SetRow label="정산·지급 내역" onClick={() => nav('/settlement')} />
      </section>

      <section className="aq-set-card" aria-labelledby="setSupport">
        <h2 id="setSupport">알림·지원</h2>
        <SetRow label="알림" value={unread ? `새 소식 ${unread}건` : undefined} onClick={() => nav('/notifications')} />
        <SetRow label="문의" onClick={() => nav('/inquiries')} />
        <SetRow label="공지사항" onClick={() => nav('/notices')} />
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
    </div>
  );
}
