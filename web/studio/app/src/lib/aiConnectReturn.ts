/**
 * AI 앱 연결(OAuth 승인·기기 연결 화면, crates/core/src/mcp)에서 로그인하러 온 경우 그 화면으로 돌려보낸다.
 * 서버 화면이 /login?next=/api/oauth/authorize?... 로 보낸다. 앱 안 브라우저(Claude·ChatGPT 모바일)는
 * 새 창을 열지 못해 같은 창에서 로그인하므로, 돌아갈 곳을 탭 저장소(sessionStorage)에 두고
 * Google·Apple 로그인으로 다른 사이트를 다녀와도 로그인 뒤에 꺼낸다.
 */
const KEY = 'aq.aiConnectReturn';

/** 돌아갈 수 있는 곳: 이 사이트의 AI 연결 승인 화면뿐 */
export function aiConnectPath(raw: unknown): string | undefined {
  if (typeof raw !== 'string' || raw.length > 4096) return undefined;
  if (!/^\/api\/oauth\/(?:authorize\?|device(?:\?|$))/.test(raw)) return undefined;
  if (raw.includes('//') || /[\\\u0000- \u007f]/.test(raw)) return undefined;
  return raw;
}

/** 주소의 ?next= 가 AI 연결 화면이면 기억한다 */
export function rememberAiConnectReturn(search: string): void {
  const path = aiConnectPath(new URLSearchParams(search).get('next'));
  if (!path) return;
  try { sessionStorage.setItem(KEY, path); } catch { /* 저장소를 못 쓰면 예전처럼 홈으로 */ }
}

/** 기억한 곳을 한 번만 꺼낸다 */
export function takeAiConnectReturn(): string | undefined {
  try {
    const path = sessionStorage.getItem(KEY);
    sessionStorage.removeItem(KEY);
    return aiConnectPath(path);
  } catch {
    return undefined;
  }
}
