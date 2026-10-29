// Google·Apple 로그인 — 서버 crates/core/src/oauth.rs (OpenID Connect).
// 시작하면 서버가 제공자 주소를 주고, 제공자는 /api/auth/oauth/{provider}/callback으로 돌아온 뒤
// 서버가 로그인시켜 원래 화면(실패면 /login?oauth_error=코드)으로 보낸다.
import { req } from './http';
import { LEGAL_META, TERMS_VERSION } from '../lib/legal';

export type OAuthProvider = 'google' | 'apple';
export const OAUTH_LABEL: Record<OAuthProvider, string> = { google: 'Google', apple: 'Apple' };

let configured: Promise<Record<OAuthProvider, boolean>> | null = null;
/** 설정이 끝난 제공자만 버튼을 보여 준다 (키를 넣기 전에는 모두 false). */
export function oauthProviders(): Promise<Record<OAuthProvider, boolean>> {
  configured ??= req<Record<OAuthProvider, boolean>>('/api/auth/oauth/providers', { quiet401: true, quietServer: true })
    .catch(() => { configured = null; return { google: false, apple: false }; });
  return configured;
}

/** 제공자 화면으로 이동한다. signup: 가입 화면에서 필수 약관에 동의한 경우만 true. */
export async function startOAuth(provider: OAuthProvider, opts: { mode?: 'login' | 'link'; returnTo?: string; signup?: boolean } = {}): Promise<void> {
  const r = await req<{ url: string }>(`/api/auth/oauth/${provider}/start`, {
    method: 'POST', quiet401: opts.mode !== 'link',
    body: {
      mode: opts.mode ?? 'login', return_to: opts.returnTo ?? null, signup: opts.signup ?? false,
      // 가입이면 동의한 약관·고지한 개인정보 처리방침의 판 (서버가 기록·봉인)
      terms_version: opts.signup ? TERMS_VERSION : null, privacy_notice_version: opts.signup ? LEGAL_META.privacy.version : null,
    },
  });
  window.location.assign(r.url);
}

export interface OAuthIdentity { id: string; provider: OAuthProvider; email: string | null; created_at: string; last_used_at: string | null }
export interface OAuthIdentities { items: OAuthIdentity[]; providers: Record<OAuthProvider, boolean>; has_password: boolean }

export const oauthIdentities = () => req<OAuthIdentities>('/api/auth/oauth/identities');
export const unlinkOAuth = (id: string) => req<OAuthIdentities>(`/api/auth/oauth/identities/${encodeURIComponent(id)}`, { method: 'DELETE' });

/** 돌아온 주소의 ?oauth_error= / ?oauth_linked= 를 한 번 읽고 주소에서 지운다. */
export function takeOAuthResult(): { error?: string; linked?: OAuthProvider } {
  const q = new URLSearchParams(window.location.search);
  const error = q.get('oauth_error') ?? undefined;
  const linked = (q.get('oauth_linked') as OAuthProvider | null) ?? undefined;
  if (error || linked) {
    q.delete('oauth_error');
    q.delete('oauth_linked');
    const rest = q.toString();
    window.history.replaceState(window.history.state, '', window.location.pathname + (rest ? `?${rest}` : '') + window.location.hash);
  }
  return { error, linked };
}
