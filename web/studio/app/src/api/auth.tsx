import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from 'react';
import { api, setCurrentOrg, type User, type Org } from './client';

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
