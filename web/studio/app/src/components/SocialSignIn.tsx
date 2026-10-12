// "Google로 계속하기" / "Apple로 계속하기" — 각 사의 버튼 가이드(로고·문구·색)를 따른다.
// 서버에 키가 설정된 제공자만 보인다(설정 전에는 아무것도 그리지 않음).
import { useEffect, useState } from 'react';
import { MOCK } from '../api/client';
import { errorMessage } from '../api/errors';
import { oauthProviders, startOAuth, type OAuthProvider } from '../api/oauth';

function GoogleLogo() {
  return (
    <svg width="18" height="18" viewBox="0 0 48 48" aria-hidden="true">
      <path fill="#EA4335" d="M24 9.5c3.54 0 6.71 1.22 9.21 3.6l6.85-6.85C35.9 2.38 30.47 0 24 0 14.62 0 6.51 5.38 2.56 13.22l7.98 6.19C12.43 13.72 17.74 9.5 24 9.5z" />
      <path fill="#4285F4" d="M46.98 24.55c0-1.57-.15-3.09-.38-4.55H24v9.02h12.94c-.58 2.96-2.26 5.48-4.78 7.18l7.73 6c4.51-4.18 7.09-10.36 7.09-17.65z" />
      <path fill="#FBBC05" d="M10.53 28.59c-.48-1.45-.76-2.99-.76-4.59s.27-3.14.76-4.59l-7.98-6.19C.92 16.46 0 20.12 0 24c0 3.88.92 7.54 2.56 10.78l7.97-6.19z" />
      <path fill="#34A853" d="M24 48c6.48 0 11.93-2.13 15.89-5.81l-7.73-6c-2.15 1.45-4.92 2.3-8.16 2.3-6.26 0-11.57-4.22-13.47-9.91l-7.98 6.19C6.51 42.62 14.62 48 24 48z" />
    </svg>
  );
}

function AppleLogo() {
  return (
    <svg width="16" height="18" viewBox="0 0 814 1000" aria-hidden="true" fill="currentColor">
      <path d="M788.1 340.9c-5.8 4.5-108.2 62.2-108.2 190.5 0 148.4 130.3 200.9 134.2 202.2-.6 3.2-20.7 71.9-68.7 141.9-42.8 61.6-87.5 123.1-155.5 123.1s-85.5-39.5-164-39.5c-76.5 0-103.7 40.8-165.9 40.8s-105.6-57-155.5-127C46.7 790.7 0 663 0 541.8c0-194.4 126.4-297.5 250.8-297.5 66.1 0 121.2 43.4 162.7 43.4 39.5 0 101.1-46 176.3-46 28.5 0 130.9 2.6 198.3 99.2zm-234-181.5c31.1-36.9 53.1-88.1 53.1-139.3 0-7.1-.6-14.3-1.9-20.1-50.6 1.9-110.8 33.7-147.1 75.8-28.5 32.4-55.1 83.6-55.1 135.5 0 7.8 1.3 15.6 1.9 18.1 3.2.6 8.4 1.3 13.6 1.3 45.4 0 102.5-30.4 135.5-71.3z" />
    </svg>
  );
}

const LABEL: Record<OAuthProvider, string> = { google: 'Google로 계속하기', apple: 'Apple로 계속하기' };

/**
 * signup: 가입 화면 — 필수 약관 동의가 끝났는지 `canSignup`으로 확인한 뒤에만 시작한다.
 * 로그인 화면에서 처음 온 계정은 서버가 가입 화면으로 보낸다.
 */
export function SocialSignIn({ signup = false, canSignup, onError, returnTo }: {
  signup?: boolean; canSignup?: () => boolean; onError: (message: string) => void; returnTo?: string;
}) {
  const [enabled, setEnabled] = useState<OAuthProvider[]>([]);
  const [busy, setBusy] = useState<OAuthProvider | null>(null);

  useEffect(() => {
    if (MOCK) return;
    let alive = true;
    void oauthProviders().then(p => { if (alive) setEnabled((['google', 'apple'] as const).filter(k => p[k])); });
    return () => { alive = false; };
  }, []);

  if (enabled.length === 0) return null;

  async function go(p: OAuthProvider) {
    if (busy) return;
    if (signup && canSignup && !canSignup()) return;
    setBusy(p);
    try {
      await startOAuth(p, { signup, returnTo });
    } catch (err) {
      onError(errorMessage(err, '잠시 후 다시 시도해 주세요.'));
      setBusy(null);
    }
  }

  return (
    <div className="aq-social">
      <div className="aq-social-divider" role="separator"><span>또는</span></div>
      {enabled.map(p => (
        <button
          key={p} type="button" className={`aq-social-btn is-${p}`} onClick={() => void go(p)}
          disabled={busy !== null} aria-busy={busy === p}
        >
          {p === 'google' ? <GoogleLogo /> : <AppleLogo />}
          <span>{busy === p ? '이동하는 중' : LABEL[p]}</span>
        </button>
      ))}
    </div>
  );
}
