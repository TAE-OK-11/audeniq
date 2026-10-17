import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from 'react';
import { ApiError, MOCK, api, setCurrentOrg, type User, type Org } from './client';
import { readJSON, writeJSON } from '../lib/storage';

/** 마지막으로 고른 작업 공간 (다른 팀에 초대받아 들어간 경우) */
const ORG_KEY = 'org';

interface AuthState {
  user: User | null;
  org: Org | null;
  orgs: Org[];
  loading: boolean;
  login: (email: string, password: string) => Promise<void>;
  loginPasskey: (email: string) => Promise<void>;
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
    // 목록을 한 번 못 받았다고 '작업 공간 없음'으로 굳지 않게 잠시 뒤 다시 받는다
    // (그대로 두면 모든 요청이 NO_ORG로 실패하고 다시 로그인하는 수밖에 없었다)
    let o: Org[] = [];
    for (let attempt = 0; attempt < 3; attempt++) {
      try { o = await api.listOrgs(); break; } catch {
        if (attempt < 2) await new Promise(r => window.setTimeout(r, 800 * (attempt + 1)));
      }
    }
    setOrgs(o);
    const saved = readJSON<string | null>(ORG_KEY, null);
    setOrg(o.find(x => x.id === saved) ?? o[0] ?? null);
  }, [setOrg]);
  // 작업 공간을 바꾸면 화면의 모든 데이터(발매·정산·알림)를 그 공간 기준으로 다시 받는다
  const selectOrg = useCallback((o: Org) => {
    writeJSON(ORG_KEY, o.id);
    window.location.reload();
  }, []);

  useEffect(() => {
    let cancelled = false;
    // 로그인 안 됨(401)만 로그인 화면으로 — 잠깐의 서버·네트워크 오류로 멀쩡한 세션이 로그아웃처럼 보였다.
    // 그 밖의 오류는 잠시 쉬었다 두 번 더 확인한다 (그래도 안 되면 로그인 화면, 서버 오류 창은 SystemStatus가 띄운다)
    const meWithRetry = async (): Promise<User> => {
      for (let i = 0; ; i++) {
        try { return await api.me(); } catch (e) {
          if (cancelled || i >= 2 || (e instanceof ApiError && (e.status === 401 || e.status === 403))) throw e;
          await new Promise(r => setTimeout(r, 800 * 2 ** i));
        }
      }
    };
    meWithRetry()
      .then(async u => {
        if (cancelled) return;
        setUser(u);
        await loadOrgs();
      })
      .catch(() => { /* 세션 없음 — 로그인 화면으로 */ })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [loadOrgs]);

  // 실서버: 로그인한 작업 공간의 포털 데이터를 불러오고 알림을 주기적으로 새로 받는다
  useEffect(() => {
    if (MOCK || !user || !org) return;
    let stop: (() => void) | undefined;
    let cancelled = false;
    void import('../store/portalSync').then(m => { if (!cancelled) stop = m.startPortalSync(); });
    return () => { cancelled = true; stop?.(); };
  }, [user, org]);

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

  const loginPasskey = useCallback(async (email: string) => {
    const u = await api.loginPasskey(email);
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
    user, org, orgs, loading, login, loginPasskey, signup, logout, selectOrg,
  }), [user, org, orgs, loading, login, loginPasskey, signup, logout, selectOrg]);

  return <AuthCtx.Provider value={value}>{children}</AuthCtx.Provider>;
}

export function useAuth(): AuthState {
  const ctx = useContext(AuthCtx);
  if (!ctx) throw new Error('useAuth must be used within AuthProvider');
  return ctx;
}
