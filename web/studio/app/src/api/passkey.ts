// 패스키(WebAuthn) — 이메일을 입력한 뒤 비밀번호 대신 기기의 지문·얼굴·PIN으로 로그인한다.
// 서버: crates/core/src/passkey.rs. 비밀키는 기기를 떠나지 않고 서버는 공개키만 받는다.
import { req, setCsrf } from './http';
import { ApiError } from './errors';

const enc = (b: ArrayBuffer | Uint8Array): string => {
  const bytes = b instanceof Uint8Array ? b : new Uint8Array(b);
  let s = '';
  for (const x of bytes) s += String.fromCharCode(x);
  return btoa(s).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
};
const dec = (s: string): Uint8Array<ArrayBuffer> => {
  const b = atob(s.replace(/-/g, '+').replace(/_/g, '/') + '==='.slice((s.length + 3) % 4));
  const out = new Uint8Array(new ArrayBuffer(b.length));
  for (let i = 0; i < b.length; i++) out[i] = b.charCodeAt(i);
  return out;
};

export function passkeySupported(): boolean {
  return typeof window !== 'undefined' && typeof window.PublicKeyCredential === 'function' && !!navigator.credentials;
}

export interface LoginCeremonyOptions { signal?: AbortSignal; conditional?: boolean }
export async function conditionalPasskeySupported(): Promise<boolean> {
  try {
    return passkeySupported() && typeof PublicKeyCredential.isConditionalMediationAvailable === 'function'
      && await PublicKeyCredential.isConditionalMediationAvailable();
  } catch { return false; }
}

let active: AbortController | undefined;
export function abortPasskeyCeremony(): void { active?.abort(); }
function ceremony(signal?: AbortSignal) {
  abortPasskeyCeremony();
  const controller = new AbortController();
  active = controller;
  const abort = () => controller.abort();
  signal?.addEventListener('abort', abort, { once: true });
  if (signal?.aborted) abort();
  window.addEventListener('pagehide', abort);
  return { signal: controller.signal, done() {
    signal?.removeEventListener('abort', abort);
    window.removeEventListener('pagehide', abort);
    if (active === controller) active = undefined;
  } };
}

/** Registration options already contain the authenticated user's complete credential
 * snapshot. List row UUIDs and generic 401s are never safe inputs to the Signal API.
 * This optional request creates an unused challenge; limits/failures simply skip sync.
 */
async function signalAccepted(rpId: string, userId: string | null, credentialId: string): Promise<void> {
  try {
    const all = typeof PublicKeyCredential.signalAllAcceptedCredentials === 'function';
    const unknown = typeof PublicKeyCredential.signalUnknownCredential === 'function';
    if (!userId || (!all && !unknown)) return;
    const { public_key: pk } = await req<RegisterOptions>('/api/auth/passkeys/options', {
      method: 'POST', quiet401: true, quietServer: true,
    });
    if (pk.rp.id !== rpId || pk.user.id !== userId) return;
    const ids = pk.excludeCredentials.map(c => c.id);
    if (all) {
      await PublicKeyCredential.signalAllAcceptedCredentials({ rpId, userId, allAcceptedCredentialIds: ids });
    } else if (unknown && !ids.includes(credentialId)) {
      // Only a fresh authenticated snapshot can prove this credential was removed.
      await PublicKeyCredential.signalUnknownCredential({ rpId, credentialId });
    }
  } catch { /* Best effort; never block login. */ }
}

/** 기기 창에서 취소·시간 초과는 오류 대신 안내로 바꾼다. */
function deviceError(e: unknown, registering: boolean): never {
  if (e instanceof ApiError) throw e;
  const name = e instanceof DOMException ? e.name : '';
  if (name === 'InvalidStateError' && registering) throw new ApiError('이 기기에는 이미 패스키를 등록했어요.', 400, 'PASSKEY_ALREADY_REGISTERED');
  if (name === 'SecurityError') throw new ApiError('이 주소에서는 패스키를 쓸 수 없어요. 운영 설정(PASSKEY_RP_ID)을 확인해 주세요.', 400, 'PASSKEY_UNSUPPORTED');
  if (name === 'NotAllowedError' || name === 'AbortError') throw new ApiError('패스키 사용을 취소했거나 시간이 지났어요. 다시 시도해 주세요.', 400, 'PASSKEY_CANCELLED');
  throw new ApiError('이 브라우저나 기기에서는 패스키를 쓸 수 없어요.', 400, 'PASSKEY_UNSUPPORTED');
}

interface LoginOptions { challenge_id: string; public_key: { challenge: string; rpId: string; timeout: number } }

/** 로그인: 세션이 만들어지면 `{user_id, csrf_token}`. 이메일은 이 패스키의 계정이어야 한다. */
export async function passkeyLogin(email: string, options: LoginCeremonyOptions = {}): Promise<{ user_id: string; csrf_token: string }> {
  if (!passkeySupported()) deviceError(null, false);
  const flow = ceremony(options.signal);
  try {
    const conditional = options.conditional && await conditionalPasskeySupported();
    if (flow.signal.aborted) deviceError(new DOMException('', 'AbortError'), false);
    const o = await req<LoginOptions>('/api/auth/passkey/options', { method: 'POST', quiet401: true, quietServer: !!conditional, signal: flow.signal });
    let cred: PublicKeyCredential | null;
    try {
      if (flow.signal.aborted) deviceError(new DOMException('', 'AbortError'), false);
      cred = await navigator.credentials.get({
        signal: flow.signal,
        ...(conditional ? { mediation: 'conditional' as const } : {}),
        publicKey: {
          challenge: dec(o.public_key.challenge), rpId: o.public_key.rpId, timeout: o.public_key.timeout,
          userVerification: 'required', allowCredentials: [],
        },
      }) as PublicKeyCredential | null;
    } catch (e) { deviceError(e, false); }
    if (!cred) deviceError(null, false);
    const r = cred.response as AuthenticatorAssertionResponse;
    try {
      if (flow.signal.aborted) deviceError(new DOMException('', 'AbortError'), false);
      const result = await req<{ user_id: string; csrf_token: string }>('/api/auth/passkey/login', {
        method: 'POST', quiet401: true, signal: flow.signal,
        body: {
          challenge_id: o.challenge_id, email: email.trim() || null,
          credential_id: enc(cred.rawId), client_data_json: enc(r.clientDataJSON),
          authenticator_data: enc(r.authenticatorData), signature: enc(r.signature),
          user_handle: r.userHandle ? enc(r.userHandle) : null,
        },
      });
      if (flow.signal.aborted) deviceError(new DOMException('', 'AbortError'), false);
      setCsrf(result.csrf_token);
      void signalAccepted(o.public_key.rpId, r.userHandle ? enc(r.userHandle) : null, enc(cred.rawId));
      return result;
    } catch (e) {
      if (e instanceof ApiError && e.status === 401) {
        throw new ApiError('패스키를 확인하지 못했어요. 입력한 이메일의 계정에 등록한 패스키인지 확인해 주세요.', 401, 'PASSKEY_LOGIN_FAILED');
      }
      throw e;
    }
  } finally { flow.done(); }
}

export interface Passkey { id: string; name: string; synced: boolean; created_at: string; last_used_at: string | null }
interface PasskeyList { items: Passkey[]; limit: number }

export const listPasskeys = () => req<PasskeyList>('/api/auth/passkeys');
export const removePasskey = (id: string) => req<PasskeyList>(`/api/auth/passkeys/${encodeURIComponent(id)}`, { method: 'DELETE' });

interface RegisterOptions {
  challenge_id: string;
  public_key: Omit<PublicKeyCredentialCreationOptions, 'challenge' | 'user' | 'excludeCredentials'> & {
    challenge: string; user: { id: string; name: string; displayName: string }; excludeCredentials: { type: 'public-key'; id: string }[];
  };
}

/** 패스키 추가: 지금 비밀번호를 한 번 더 확인한다. */
export async function addPasskey(currentPassword: string, name: string, signal?: AbortSignal): Promise<PasskeyList> {
  if (!passkeySupported()) deviceError(null, true);
  const flow = ceremony(signal);
  try {
    const o = await req<RegisterOptions>('/api/auth/passkeys/options', { method: 'POST', signal: flow.signal });
    const pk = o.public_key;
    let cred: PublicKeyCredential | null;
    try {
      if (flow.signal.aborted) deviceError(new DOMException('', 'AbortError'), true);
      cred = await navigator.credentials.create({
        signal: flow.signal,
        publicKey: {
          ...pk,
          challenge: dec(pk.challenge),
          user: { ...pk.user, id: dec(pk.user.id) },
          excludeCredentials: pk.excludeCredentials.map(c => ({ type: c.type, id: dec(c.id) })),
        },
      }) as PublicKeyCredential | null;
    } catch (e) { deviceError(e, true); }
    if (!cred) deviceError(null, true);
    const r = cred.response as AuthenticatorAttestationResponse;
    const spki = typeof r.getPublicKey === 'function' ? r.getPublicKey() : null;
    if (!spki) deviceError(null, true);
    if (flow.signal.aborted) deviceError(new DOMException('', 'AbortError'), true);
    return await req<PasskeyList>('/api/auth/passkeys/register', {
      method: 'POST', signal: flow.signal,
      body: {
        challenge_id: o.challenge_id, current_password: currentPassword, name: name.trim(),
        credential_id: enc(cred.rawId), client_data_json: enc(r.clientDataJSON),
        authenticator_data: enc(r.getAuthenticatorData()), public_key: enc(spki), alg: r.getPublicKeyAlgorithm(),
      },
    });
  } finally { flow.done(); }
}

/** 기기 이름 기본값 */
export function defaultPasskeyName(): string {
  const ua = navigator.userAgent;
  if (/iPhone/.test(ua)) return 'iPhone';
  if (/iPad/.test(ua)) return 'iPad';
  if (/Android/.test(ua)) return 'Android';
  if (/Mac OS X/.test(ua)) return 'Mac';
  if (/Windows/.test(ua)) return 'Windows';
  return '이 기기';
}
