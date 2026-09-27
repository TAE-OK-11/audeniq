// 로그인 상태 — 스튜디오와 같은 계정·세션 API(/api/auth/*, /api/me)를 쓴다.
// 스태프 여부는 로그인 후 AdminApp이 /api/staff/me로 확인한다.
import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from 'react';
import { bootstrapCsrf, hasCsrf, req, setCsrf } from './api/http';
import { ApiError } from './api/errors';
import { MOCK } from './lib/mode';

export interface User { id: string; email: string }

interface AuthState {
  user: User | null;
  loading: boolean;
  login: (email: string, password: string) => Promise<void>;
  logout: () => Promise<void>;
}

const ACCOUNT_KEY = 'aq.admin.account';
const remember = (email: string) => { try { localStorage.setItem(ACCOUNT_KEY, email); } catch { /* 저장소 없음 */ } };
const recall = () => { try { return localStorage.getItem(ACCOUNT_KEY) ?? ''; } catch { return ''; } };
const forget = () => { try { localStorage.removeItem(ACCOUNT_KEY); } catch { /* 저장소 없음 */ } };

const remote = {
  async me(): Promise<User> {
    const me = await req<{ user_id: string }>('/api/me', { quiet401: true });
    if (!hasCsrf()) await bootstrapCsrf();
    return { id: me.user_id, email: recall() };
  },
  async login(email: string, password: string): Promise<User> {
    const r = await req<{ user_id: string; csrf_token: string }>('/api/auth/login', { method: 'POST', body: { email, password }, quiet401: true });
    setCsrf(r.csrf_token);
    remember(email);
    return { id: r.user_id, email };
  },
  async logout(): Promise<void> {
    try { await req('/api/auth/logout', { method: 'POST', quiet401: true }); } finally { setCsrf(''); forget(); }
  },
};

// 체험 모드: 아무 이메일로나 로그인 (브라우저 탭 안에서만 유지)
const demo = {
  async me(): Promise<User> {
    const email = sessionStorage.getItem(ACCOUNT_KEY);
    if (!email) throw new ApiError('로그인이 필요해요.', 401, 'UNAUTHENTICATED');
    return { id: 'staff-me-0001', email };
  },
  async login(email: string): Promise<User> {
    sessionStorage.setItem(ACCOUNT_KEY, email);
    return { id: 'staff-me-0001', email };
  },
  async logout(): Promise<void> { sessionStorage.removeItem(ACCOUNT_KEY); },
};

const api = MOCK ? demo : remote;
const AuthCtx = createContext<AuthState | null>(null);

export function AuthProvider({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<User | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let alive = true;
    api.me()
      .then(u => { if (alive) setUser(u); })
      .catch(() => { /* 세션 없음 — 로그인 화면으로 */ })
      .finally(() => { if (alive) setLoading(false); });
    return () => { alive = false; };
  }, []);

  const login = useCallback(async (email: string, password: string) => {
    setUser(await api.login(email.trim(), password));
  }, []);

  const logout = useCallback(async () => {
    try { await api.logout(); } finally { setUser(null); }
  }, []);

  const value = useMemo(() => ({ user, loading, login, logout }), [user, loading, login, logout]);
  return <AuthCtx.Provider value={value}>{children}</AuthCtx.Provider>;
}

export function useAuth(): AuthState {
  const ctx = useContext(AuthCtx);
  if (!ctx) throw new Error('useAuth must be used within AuthProvider');
  return ctx;
}
