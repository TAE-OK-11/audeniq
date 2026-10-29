import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { abortPasskeyCeremony, addPasskey, conditionalPasskeySupported, passkeyLogin } from './passkey';
import { req } from './http';
vi.mock('./http', () => ({ req: vi.fn(), setCsrf: vi.fn() }));
const request = vi.mocked(req);
const get = vi.fn();
const create = vi.fn();
const all = vi.fn().mockResolvedValue(undefined);
const unknown = vi.fn().mockResolvedValue(undefined);
const bytes = new Uint8Array([1, 2]).buffer;
const credential = { rawId: bytes, response: { clientDataJSON: bytes, authenticatorData: bytes, signature: bytes, userHandle: bytes } };
const options = { challenge_id: 'challenge', public_key: { challenge: 'AQI', rpId: 'example.com', timeout: 300000 } };
const registration = { challenge_id: 'register', public_key: { challenge: 'AQI', rp: { id: 'example.com' }, user: { id: 'AQI', name: 'name', displayName: 'name' }, excludeCredentials: [{ type: 'public-key', id: 'AQI' }] } };
beforeEach(() => {
  vi.stubGlobal('PublicKeyCredential', Object.assign(function () {}, { isConditionalMediationAvailable: vi.fn().mockResolvedValue(true), signalAllAcceptedCredentials: all, signalUnknownCredential: unknown }));
  Object.defineProperty(navigator, 'credentials', { configurable: true, value: { get, create } });
  request.mockImplementation(async path => {
    if (path === '/api/auth/passkey/options') return options;
    if (path === '/api/auth/passkey/login') return { user_id: 'user', csrf_token: 'csrf' };
    if (path === '/api/auth/passkeys/options') return registration;
    return { items: [], limit: 10 };
  });
  get.mockResolvedValue(credential);
});
afterEach(() => { abortPasskeyCeremony(); vi.clearAllMocks(); vi.unstubAllGlobals(); });
it('uses conditional discovery without an email and signals a complete snapshot', async () => {
  await expect(passkeyLogin('', { conditional: true })).resolves.toMatchObject({ user_id: 'user' });
  expect(get.mock.calls[0][0]).toMatchObject({ mediation: 'conditional', publicKey: { allowCredentials: [] } });
  expect(request).toHaveBeenCalledWith('/api/auth/passkey/login', expect.objectContaining({ body: expect.objectContaining({ email: null, user_handle: 'AQI' }) }));
  await vi.waitFor(() => expect(all).toHaveBeenCalledWith({ rpId: 'example.com', userId: 'AQI', allAcceptedCredentialIds: ['AQI'] }));
});
it('falls back to modal options when conditional mediation is unavailable', async () => {
  vi.mocked(PublicKeyCredential.isConditionalMediationAvailable).mockResolvedValue(false);
  expect(await conditionalPasskeySupported()).toBe(false);
  await passkeyLogin('');
  expect(get.mock.calls[0][0].mediation).toBeUndefined();
});
it('treats capability detection failure as unsupported', async () => {
  vi.mocked(PublicKeyCredential.isConditionalMediationAvailable).mockRejectedValue(new Error('unsupported'));
  expect(await conditionalPasskeySupported()).toBe(false);
});
it('aborts a pending device request and retains the cancellation message', async () => {
  get.mockImplementation(({ signal }) => new Promise((_, reject) => signal.addEventListener('abort', () => reject(new DOMException('', 'AbortError')))));
  const result = passkeyLogin('');
  const checked = expect(result).rejects.toMatchObject({ code: 'PASSKEY_CANCELLED', message: '패스키 사용을 취소했거나 시간이 지났어요. 다시 시도해 주세요.' });
  await vi.waitFor(() => expect(get).toHaveBeenCalled());
  abortPasskeyCeremony();
  await checked;
  expect(request.mock.calls.some(([path]) => path === '/api/auth/passkey/login')).toBe(false);
});
it('does not invoke the device after cancellation during challenge fetch', async () => {
  let resolve!: (value: unknown) => void;
  request.mockImplementationOnce(() => new Promise(r => { resolve = r; }));
  const controller = new AbortController();
  const checked = expect(passkeyLogin('', { signal: controller.signal })).rejects.toMatchObject({ code: 'PASSKEY_CANCELLED' });
  controller.abort();
  resolve(options);
  await checked;
  expect(get).not.toHaveBeenCalled();
});
it('never blocks login on Signal API rejection or signals unknown on a generic 401', async () => {
  all.mockRejectedValueOnce(new Error('provider unavailable'));
  await expect(passkeyLogin('')).resolves.toMatchObject({ user_id: 'user' });
  await vi.waitFor(() => expect(all).toHaveBeenCalled());
  expect(unknown).not.toHaveBeenCalled();
});
it('keeps registration exclusions and duplicate-device error mapping', async () => {
  create.mockRejectedValueOnce(new DOMException('', 'InvalidStateError'));
  await expect(addPasskey('password', 'device')).rejects.toMatchObject({ code: 'PASSKEY_ALREADY_REGISTERED', message: '이 기기에는 이미 패스키를 등록했어요.' });
  expect(create.mock.calls[0][0].publicKey.excludeCredentials[0].id).toEqual(new Uint8Array(bytes));
});
it('does not submit registration when getPublicKey returns null', async () => {
  create.mockResolvedValueOnce({ rawId: bytes, response: { getPublicKey: () => null } });
  await expect(addPasskey('password', 'device')).rejects.toMatchObject({ code: 'PASSKEY_UNSUPPORTED' });
  expect(request.mock.calls.some(([path]) => path === '/api/auth/passkeys/register')).toBe(false);
});
it('only signals unknown after an authenticated snapshot proves removal', async () => {
  Object.defineProperty(PublicKeyCredential, 'signalAllAcceptedCredentials', { value: undefined });
  request.mockImplementation(async path => {
    if (path === '/api/auth/passkey/options') return options;
    if (path === '/api/auth/passkey/login') return { user_id: 'user', csrf_token: 'csrf' };
    return { ...registration, public_key: { ...registration.public_key, excludeCredentials: [] } };
  });
  await passkeyLogin('');
  await vi.waitFor(() => expect(unknown).toHaveBeenCalledWith({ rpId: 'example.com', credentialId: 'AQI' }));
});
it('preserves timeout mapping', async () => {
  get.mockRejectedValueOnce(new DOMException('', 'NotAllowedError'));
  await expect(passkeyLogin('')).rejects.toMatchObject({ code: 'PASSKEY_CANCELLED', message: '패스키 사용을 취소했거나 시간이 지났어요. 다시 시도해 주세요.' });
});
it('aborts registration on pagehide', async () => {
  create.mockImplementationOnce(({ signal }) => new Promise((_, reject) => signal.addEventListener('abort', () => reject(new DOMException('', 'AbortError')))));
  const checked = expect(addPasskey('password', 'device')).rejects.toMatchObject({ code: 'PASSKEY_CANCELLED' });
  await vi.waitFor(() => expect(create).toHaveBeenCalled());
  window.dispatchEvent(new Event('pagehide'));
  await checked;
});
