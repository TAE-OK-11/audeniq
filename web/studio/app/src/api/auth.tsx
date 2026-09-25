import { createContext, useContext, useEffect, useState, type ReactNode } from 'react';
import { api, type User, type Org } from './client';

interface AuthState {
  user: User | null;
  org: Org | null;
  orgs: Org[];
  loading: boolean;
  login: (email: string, password: string) => Promise<void>;
  logout: () => Promise<void>;
  selectOrg: (org: Org) => void;
}

const AuthCtx = createContext<AuthState | null>(null);

export function AuthProvider({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<User | null>(null);
  const [orgs, setOrgs] = useState<Org[]>([]);
  const [org, setOrg] = useState<Org | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    api.me()
      .then(async u => {
        setUser(u);
        const o = await api.listOrgs();
        setOrgs(o);
        if (o.length > 0) setOrg(o[0]);
      })
      .catch(() => {})
      .finally(() => setLoading(false));
  }, []);

  async function login(email: string, password: string) {
    const u = await api.login(email, password);
    setUser(u);
    const o = await api.listOrgs();
    setOrgs(o);
    if (o.length > 0) setOrg(o[0]);
  }

  async function logout() {
    await api.logout();
    setUser(null);
    setOrg(null);
    setOrgs([]);
  }

  return (
    <AuthCtx.Provider value={{ user, org, orgs, loading, login, logout, selectOrg: setOrg }}>
      {children}
    </AuthCtx.Provider>
  );
}

export function useAuth(): AuthState {
  const ctx = useContext(AuthCtx);
  if (!ctx) throw new Error('useAuth must be used within AuthProvider');
  return ctx;
}
