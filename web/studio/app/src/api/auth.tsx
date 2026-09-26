import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from 'react';
import { MOCK, api, setCurrentOrg, type User, type Org } from './client';

interface AuthState {
  user: User | null;
  org: Org | null;
  orgs: Org[];
  loading: boolean;
  login: (email: string, password: string) => Promise<void>;
  signup: (email: string, password: string) => Promise<void>;
  logout: () => Promise<void>;
  selectOrg: (org: Org) => void;
}

const AuthCtx = createContext<AuthState | null>(null);

export function AuthProvider({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<User | null>(null);
  const [orgs, setOrgs] = useState<Org[]>([]);
  const [org, setOrgState] = useState<Org | null>(null);
  const [loading, setLoading] = useState(true);

  const setOrg = useCallback((o: Org | null) => {
    setCurrentOrg(o?.id ?? '');
    setOrgState(o);
  }, []);

  const loadOrgs = useCallback(async () => {
    const o = await api.listOrgs().catch(() => [] as Org[]);
    setOrgs(o);
    setOrg(o[0] ?? null);
  }, [setOrg]);

  useEffect(() => {
    let cancelled = false;
    api.me()
      .then(async u => {
        if (cancelled) return;
        setUser(u);
        await loadOrgs();
      })
      .catch(() => { /* 세션 없음 — 로그인 화면으로 */ })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [loadOrgs]);

  // 다른 탭에서 로그아웃/로그인하면 이 탭도 따라간다 (목 모드 세션은 localStorage 공유)
  useEffect(() => {
    if (!MOCK) return;
    const onStorage = (e: StorageEvent) => {
      if (!e.key?.endsWith('mock.session')) return;
      if (e.newValue == null) {
        setUser(null);
        setOrg(null);
      } else {
        api.me().then(async u => { setUser(u); await loadOrgs(); }).catch(() => {});
      }
    };
    window.addEventListener('storage', onStorage);
    return () => window.removeEventListener('storage', onStorage);
  }, [loadOrgs, setOrg]);

  const login = useCallback(async (email: string, password: string) => {
    const u = await api.login(email, password);
    await loadOrgs();
    setUser(u);
  }, [loadOrgs]);

  const signup = useCallback(async (email: string, password: string) => {
    const u = await api.signup(email, password);
    await loadOrgs();
    setUser(u);
  }, [loadOrgs]);

  const logout = useCallback(async () => {
    try {
      await api.logout();
    } finally {
      setUser(null);
      setOrg(null);
      setOrgs([]);
    }
  }, [setOrg]);

  const value = useMemo(() => ({
    user, org, orgs, loading, login, signup, logout, selectOrg: setOrg,
  }), [user, org, orgs, loading, login, signup, logout, setOrg]);

  return <AuthCtx.Provider value={value}>{children}</AuthCtx.Provider>;
}

export function useAuth(): AuthState {
  const ctx = useContext(AuthCtx);
  if (!ctx) throw new Error('useAuth must be used within AuthProvider');
  return ctx;
}
