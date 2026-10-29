// 비밀번호로 로그인·가입한 직후 '이 기기에 패스키를 만들까요?' — 장기적으로 비밀번호 대신 패스키로 옮겨 가게.
// Google·FIDO 패스키 안내: 방금 본인을 확인한 순간(비밀번호 로그인·가입·복구 직후)이 만들기를 권하기 가장 좋은 때이고,
// 기기 창이 뜨기 전에 무엇을 하는지(지문·얼굴·화면 잠금) 먼저 말하고, 함께 쓰는 기기에서는 만들지 말라고 알려 준다.
// 이미 패스키가 있거나, 기기가 지원하지 않거나, 최근에 '나중에'를 눌렀으면 묻지 않는다.
import { useState } from 'react';
import { Glyph } from './Glyph';
import { addPasskey, defaultPasskeyName, listPasskeys, passkeySupported } from '../api/passkey';
import { ApiError, errorMessage } from '../api/errors';
import { readJSON, writeJSON } from '../lib/storage';

const SNOOZE_KEY = 'passkey.offer.snoozedAt';
const SNOOZE_MS = 30 * 86_400_000;

/** 지금 패스키 만들기를 권할지 — 지원하는 기기이고, 이 계정에 패스키가 없고, 30일 안에 '나중에'를 누르지 않았을 때 */
export async function shouldOfferPasskey(): Promise<boolean> {
  if (!passkeySupported()) return false;
  const snoozed = readJSON<number>(SNOOZE_KEY, 0);
  if (snoozed && Date.now() - snoozed < SNOOZE_MS) return false;
  try {
    const { items } = await listPasskeys();
    return items.length === 0;
  } catch {
    return false;
  }
}

/** 로그인·가입 카드 안에 들어가는 '패스키 만들기' 단계. 만들었거나 건너뛰면 onDone */
export function PasskeyOffer({ password, onDone }: { password: string; onDone: (created: boolean) => void }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');

  const create = async () => {
    if (busy) return;
    setBusy(true);
    setError('');
    try {
      await addPasskey(password, defaultPasskeyName());
      onDone(true);
    } catch (e) {
      // 취소·이미 등록·지원 안 함은 기기 쪽 안내 문구를 그대로
      setError(e instanceof ApiError && e.code.startsWith('PASSKEY') ? e.message : errorMessage(e, '패스키를 만들지 못했어요. 잠시 후 다시 시도해 주세요.'));
      setBusy(false);
    }
  };
  const later = () => {
    writeJSON(SNOOZE_KEY, Date.now());
    onDone(false);
  };

  return (
    <div className="aq-passkey-offer">
      <ul className="aq-passkey-points">
        <li><span className="ui-icon is-sm" aria-hidden="true"><Glyph name="key" size={16} /></span>지문·얼굴·화면 잠금(PIN)으로 바로 로그인해요.</li>
        <li><span className="ui-icon is-sm" aria-hidden="true"><Glyph name="lock" size={16} /></span>비밀번호를 외우거나 입력하지 않아도 되고, 피싱에 안전해요.</li>
        <li><span className="ui-icon is-sm" aria-hidden="true"><Glyph name="user" size={16} /></span>여러 사람이 함께 쓰는 기기에서는 만들지 마세요.</li>
      </ul>
      {error && <div className="ui-callout is-danger" role="alert">{error}</div>}
      <button type="button" className={`ui-btn is-primary auth-submit${busy ? ' is-busy' : ''}`} onClick={() => void create()} disabled={busy} aria-busy={busy}>
        <Glyph name="key" size={18} />{busy ? '만드는 중' : '패스키 만들기'}
      </button>
      <button type="button" className="ui-btn is-text auth-later" onClick={later} disabled={busy}>나중에 할게요</button>
      <p className="auth-fine">계정 정보 › 로그인 › 패스키에서 언제든 만들거나 지울 수 있어요.</p>
    </div>
  );
}
