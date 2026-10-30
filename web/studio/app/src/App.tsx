import { loginRedirect } from './lib/loginRedirect';
import { rememberAiConnectReturn, takeAiConnectReturn } from './lib/aiConnectReturn';
import { Suspense, useEffect, useState, type ReactNode } from 'react';
import { BrowserRouter, Routes, Route, Navigate, useLocation, useParams } from './lib/router';
import { artistPath, artistPressPath, releasePath } from './lib/publicLinks';
import { AuthProvider, useAuth } from './api/auth';
import { ToastProvider } from './components/Toast';
import { hasPasskeyHandoff } from './lib/passkeyHandoff';
import { hasPhoneHandoff } from './lib/phoneHandoff';
import { ConfirmProvider } from './components/Confirm';
import { ErrorBoundary } from './components/ErrorBoundary';
import { SystemStatus } from './components/SystemStatus';
import { PageSkeleton } from './components/Skeleton';
import { pageLoaders, prefetchCommonRoutes } from './routes';
import './styles/tokens.css';
import './styles/design.css';
import './styles/live.css';
import './styles/enhance.css';
import './styles/controls.css';
import './styles/ui.css';
import './styles/desktop.css';
import './styles/wizard-motion.css';
import './styles/design-align.css';
import { lazyComponent } from './lib/lazyComponent';
import { preloadMoreFont } from './lib/fonts';
import { watchBusyButtons } from './lib/busyButtons';

// 배포 직후 옛 청크가 사라지면 한 번 새로고침해 새 버전을 받는다 (lib/lazyComponent)
const lazyPage = lazyComponent;

const Login = lazyPage(pageLoaders.Login, m => m.Login);
const PasskeySetup = lazyPage(pageLoaders.PasskeySetup, m => m.PasskeySetup);
const PhoneSetup = lazyPage(pageLoaders.PhoneSetup, m => m.PhoneSetup);
const Signup = lazyPage(pageLoaders.Signup, m => m.Signup);
const FindAccount = lazyPage(pageLoaders.FindAccount, m => m.FindAccount);
const Sign = lazyPage(pageLoaders.Sign, m => m.Sign);
const Terms = lazyPage(pageLoaders.Terms, m => m.Terms);
const Verify = lazyPage(pageLoaders.Verify, m => m.Verify);
const RightsReport = lazyPage(pageLoaders.RightsReport, m => m.RightsReport);
const SmartLink = lazyPage(pageLoaders.SmartLink, m => m.SmartLink);
const ArtistPage = lazyPage(pageLoaders.ArtistPage, m => m.ArtistPage);
const ArtistPress = lazyPage(pageLoaders.ArtistPress, m => m.ArtistPress);
const ArtistNews = lazyPage(pageLoaders.ArtistNews, m => m.ArtistNews);
const ArtistDiscography = lazyPage(pageLoaders.ArtistDiscography, m => m.ArtistDiscography);
const ArtistStudio = lazyPage(pageLoaders.ArtistStudio, m => m.ArtistStudio);
const SplitInvite = lazyPage(pageLoaders.SplitInvite, m => m.SplitInvite);
const SettlementTerms = lazyPage(pageLoaders.Terms, m => m.SettlementTerms);
const PrivacyPolicy = lazyPage(pageLoaders.Terms, m => m.PrivacyPolicy);
const Dashboard = lazyPage(pageLoaders.Dashboard, m => m.Dashboard);
const Releases = lazyPage(pageLoaders.Releases, m => m.Releases);
const ReleaseDetail = lazyPage(pageLoaders.ReleaseDetail, m => m.ReleaseDetail);
const Application = lazyPage(pageLoaders.Application, m => m.Application);
const Upload = lazyPage(pageLoaders.Upload, m => m.Upload);
const Reports = lazyPage(pageLoaders.Reports, m => m.Reports);
const Settlement = lazyPage(pageLoaders.Settlement, m => m.Settlement);
const Contracts = lazyPage(pageLoaders.Contracts, m => m.Contracts);
const Agreement = lazyPage(pageLoaders.Agreement, m => m.Agreement);
const Rights = lazyPage(pageLoaders.Rights, m => m.Rights);
const RightsDocument = lazyPage(pageLoaders.RightsDocument, m => m.RightsDocument);
const Inquiries = lazyPage(pageLoaders.Inquiries, m => m.Inquiries);
const Notifications = lazyPage(pageLoaders.Notifications, m => m.Notifications);
const Events = lazyPage(pageLoaders.Events, m => m.Events);
const Notices = lazyPage(pageLoaders.Notices, m => m.Notices);
const NoticeDetail = lazyPage(pageLoaders.Notices, m => m.NoticeDetail);
const NotFound = lazyPage(pageLoaders.NotFound, m => m.NotFound);
const Profile = lazyPage(pageLoaders.Profile, m => m.Profile);
const AiApprove = lazyPage(pageLoaders.AiApprove, m => m.AiApprove);
const AiAction = lazyPage(pageLoaders.AiAction, m => m.AiAction);
const AiConnect = lazyPage(pageLoaders.AiConnect, m => m.AiConnect);
const AdminApp = lazyPage(pageLoaders.Admin, m => m.AdminApp);
// 머리글·메뉴 — 로그인·공개 페이지에는 필요 없어 따로 받는다 (포털 주소로 들어오면 routes가 인증 확인과 함께 미리 받는다)
const Layout = lazyPage(pageLoaders.Layout, m => m.Layout);
// 터치 기기의 선택 시트·출력 도장 — 첫 화면에는 필요 없어 한가할 때 받는다 (그 전에는 기본 선택 창이 열린다)
const SelectHost = lazyComponent(() => import('./components/PickerSurface'), m => m.SelectHost);
const PrintStampHost = lazyComponent(() => import('./lib/printStamp'), m => m.PrintStampHost);

function IdleHosts() {
  const [ready, setReady] = useState(false);
  useEffect(() => {
    const go = () => setReady(true);
    if ('requestIdleCallback' in window) { const id = window.requestIdleCallback(go, { timeout: 2000 }); return () => window.cancelIdleCallback(id); }
    const t = setTimeout(go, 1200);
    return () => clearTimeout(t);
  }, []);
  return ready ? <Suspense fallback={null}><SelectHost /><PrintStampHost /></Suspense> : null;
}

/** index.html이 그려 둔 첫 화면이 있었는지 — 있었으면 첫 BootScreen은 다시 흐려졌다 나타나지 않고 그대로 이어진다 */
let htmlBoot = !!document.querySelector('#root > .aq-boot');

function BootScreen() {
  const [still] = useState(() => htmlBoot);
  useEffect(() => { htmlBoot = false; }, []);
  return (
    <div className="aq-boot" role="status" aria-label="AUDENIQ STUDIO를 불러오는 중" style={still ? { animation: 'none' } : undefined}>
      <img src={`${import.meta.env.BASE_URL}static/AUDENIQ_Logo_Light.svg`} alt="" />
      <span className="aq-boot-bar" />
    </div>
  );
}

/** 로그인 필요 — 원래 가려던 주소를 기억해 로그인 후 되돌아간다 */
function Protected({ children }: { children: ReactNode }) {
  const { user, loading } = useAuth();
  // 로그인한 화면에는 이름·제목이 나온다 — 글꼴 두 번째 조각을 한가할 때 받아 둔다 (lib/fonts)
  useEffect(() => {
    if (!user) return;
    if ('requestIdleCallback' in window) { const id = window.requestIdleCallback(preloadMoreFont, { timeout: 1500 }); return () => window.cancelIdleCallback(id); }
    const t = setTimeout(preloadMoreFont, 800);
    return () => clearTimeout(t);
  }, [user]);
  const loc = useLocation();
  // AI 앱 연결 화면에서 로그인하러 왔으면 (Google·Apple 로그인 포함) 로그인 뒤 그 화면으로
  const [leaving, setLeaving] = useState(false);
  useEffect(() => {
    if (!user) return;
    const to = takeAiConnectReturn();
    if (to) { setLeaving(true); window.location.assign(to); }
  }, [user]);
  if (loading || leaving) return <BootScreen />;
  if (!user) return <Navigate to="/login" replace state={{ from: loc.pathname + loc.search }} />;
  return <>{children}</>;
}

/** 이미 로그인한 사용자가 인증 화면에 오면 홈으로 */
function GuestOnly({ children }: { children: ReactNode }) {
  const { user, loading } = useAuth();
  const loc = useLocation();
  rememberAiConnectReturn(loc.search);
  // 로그인·가입 화면을 보는 동안 한가할 때 홈 화면을 미리 받아 둔다 — 로그인하자마자 홈이 바로 뜬다
  useEffect(() => { prefetchCommonRoutes(); }, []);
  if (loading) return <BootScreen />;
  if (user) {
    // 막 가입했으면 휴대폰 번호 확인부터 (pages/PhoneSetup)
    if (hasPhoneHandoff()) return <Navigate to="/phone-setup" replace />;
    // 비밀번호로 막 로그인·가입했으면 홈을 거치지 않고 패스키 만들기 화면으로 (pages/PasskeySetup)
    if (hasPasskeyHandoff()) return <Navigate to="/passkey-setup" replace />;
    const from = loginRedirect(loc.state);
    return <Navigate to={from ?? '/'} replace />;
  }
  return <>{children}</>;
}

/** 다른 발매로 바로 옮겨 가면(알림 클릭 등) 앞 발매 내용·탭이 남지 않게 새로 그린다 */
/** 예전 공개 주소(/l/·/a/) → 새 주소, 쿼리·해시는 그대로 */
function MovedLink({ to }: { to: (slug: string) => string }) {
  const { slug = '' } = useParams();
  const loc = useLocation();
  return <Navigate to={`${to(slug)}${loc.search}${loc.hash ?? ''}`} replace />;
}

function ReleaseDetailRoute() {
  const { id = '' } = useParams();
  return <ReleaseDetail key={id} />;
}

/** ?edit= 값이 바뀌면 위자드 상태를 새로 시작 (새 발매 ↔ 수정 전환 시 이전 입력이 남는 문제 방지) */
function UploadRoute() {
  const loc = useLocation();
  return <Upload key={loc.search} />;
}

/** 예전 플랫폼 요청 주소 — 요청은 이제 문의 티켓이다 (?id= → 그 티켓 열기, ?tab= → 그 유형으로 새로 쓰기) */
function PlatformRequestsMoved() {
  const loc = useLocation();
  const p = new URLSearchParams(loc.search);
  const id = p.get('id');
  const to = id ? `/inquiries?open=${encodeURIComponent(id)}` : '/inquiries';
  return <Navigate to={to} replace />;
}

function PortalRoutes() {
  const loc = useLocation();
  return (
    <Layout>
      <ErrorBoundary resetKey={loc.pathname}>
        <Suspense fallback={<PageSkeleton />}>
          <Routes>
            <Route path="/" element={<Dashboard />} />
            <Route path="/releases" element={<Releases />} />
            <Route path="/releases/bulk" element={<Navigate to="/releases" replace />} />
            <Route path="/releases/:id" element={<ReleaseDetailRoute />} />
            <Route path="/releases/:id/application" element={<Application />} />
            <Route path="/upload" element={<UploadRoute />} />
            <Route path="/reports" element={<Reports />} />
            <Route path="/settlement" element={<Settlement />} />
            <Route path="/contracts" element={<Contracts />} />
            <Route path="/contracts/:id" element={<Agreement />} />
            <Route path="/rights" element={<Rights />} />
            <Route path="/rights/:id" element={<RightsDocument />} />
            <Route path="/inquiries" element={<Inquiries />} />
            <Route path="/platform-requests" element={<PlatformRequestsMoved />} />
            <Route path="/notifications" element={<Notifications />} />
            <Route path="/events" element={<Events />} />
            <Route path="/notices" element={<Notices />} />
            <Route path="/notices/:id" element={<NoticeDetail />} />
            <Route path="/support" element={<Navigate to="/inquiries" replace />} />
            <Route path="/profile" element={<Profile />} />
            <Route path="/artist" element={<ArtistStudio />} />
            <Route path="*" element={<NotFound />} />
          </Routes>
        </Suspense>
      </ErrorBoundary>
    </Layout>
  );
}

export function App() {
  // '…중' 글자로 바뀐 막힌 버튼에 처리 중 색 (lib/busyButtons)
  useEffect(() => watchBusyButtons(), []);
  return (
    <BrowserRouter>
      <AuthProvider>
        <ToastProvider>
          <ConfirmProvider>
            <IdleHosts />
            <ErrorBoundary fullPage>
              <SystemStatus>
                <Suspense fallback={<BootScreen />}>
                  <Routes>
                    <Route path="/login" element={<GuestOnly><Login /></GuestOnly>} />
                    <Route path="/signup" element={<GuestOnly><Signup /></GuestOnly>} />
                    <Route path="/find-account" element={<GuestOnly><FindAccount /></GuestOnly>} />
                    <Route path="/passkey-setup" element={<Protected><PasskeySetup /></Protected>} />
                    <Route path="/phone-setup" element={<Protected><PhoneSetup /></Protected>} />
                    {/* 권리자 서명 — 로그인 없이 받은 링크로 (본인확인 후 직접 서명) */}
                    <Route path="/sign/:token" element={<Sign />} />
                    <Route path="/terms" element={<Terms />} />
                    <Route path="/terms/settlement" element={<SettlementTerms />} />
                    <Route path="/privacy" element={<PrivacyPolicy />} />
                    {/* 출력물 진위 확인 — 로그인 없이 원본 검증코드로 */}
                    <Route path="/verify" element={<Verify />} />
                    {/* 권리침해 신고 — 계정 없이 누구나 */}
                    <Route path="/rights-report" element={<RightsReport />} />
                    {/* 스마트링크 — 발매 공유 페이지, 로그인 없이 */}
                    <Route path="/link/release/:slug" element={<SmartLink />} />
                    {/* 아티스트 소개 페이지 — 이름·소개·발매, 발매는 스마트링크로. 주소 이름은 아티스트가 고른다 */}
                    <Route path="/link/artist/:slug" element={<ArtistPage />} />
                    {/* 보도자료 — 같은 아티스트 페이지의 큐레이터·기자용 */}
                    <Route path="/link/artist/:slug/press" element={<ArtistPress />} />
                    <Route path="/link/artist/:slug/news" element={<ArtistNews />} />
                    <Route path="/link/artist/:slug/discography" element={<ArtistDiscography />} />
                    {/* 예전 주소 — 이미 나간 QR·링크가 계속 열리게 새 주소로 넘긴다 (?src= 등은 그대로) */}
                    <Route path="/l/:slug" element={<MovedLink to={releasePath} />} />
                    <Route path="/a/:slug" element={<MovedLink to={artistPath} />} />
                    <Route path="/a/:slug/press" element={<MovedLink to={artistPressPath} />} />
                    {/* 공동작업 수익 분배 요청 — 메일 링크, 수락은 로그인 후 */}
                    <Route path="/splits/:token" element={<SplitInvite />} />
                    <Route path="/terms/privacy" element={<Navigate to="/privacy" replace />} />
                    <Route path="/content-admin" element={<Navigate to="/admin/content" replace />} />
                    {/* 스태프 관리자 — 같은 로그인 세션 + 서버의 스태프 역할(/api/staff/me)로 접근 */}
                    <Route path="/admin/*" element={<Protected><AdminApp /></Protected>} />
                    {/* AI 앱 연결 승인 — 스튜디오 메뉴 없이 한 화면으로 (AI 앱의 앱 안 브라우저에서 열린다) */}
                    <Route path="/ai/connect" element={<Protected><AiConnect /></Protected>} />
                    {/* AI 앱이 보낸 권한·작업 승인 링크 — 연결 화면과 같은 틀 */}
                    <Route path="/ai/approve/:id" element={<Protected><AiApprove /></Protected>} />
                    <Route path="/ai/actions/:id" element={<Protected><AiAction /></Protected>} />
                    <Route path="/*" element={<Protected><PortalRoutes /></Protected>} />
                  </Routes>
                </Suspense>
              </SystemStatus>
            </ErrorBoundary>
          </ConfirmProvider>
        </ToastProvider>
      </AuthProvider>
    </BrowserRouter>
  );
}

