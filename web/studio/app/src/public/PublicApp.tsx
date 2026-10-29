// 공개 페이지 전용 앱 — 스마트링크·아티스트 페이지(/link/…, 예전 /l/·/a/)는 로그인·메뉴·스튜디오 화면 코드 없이 연다.
// 팬이 받는 것은 이 앱과 공개 페이지가 쓰는 스타일(styles/public.css)뿐이다 (main.tsx가 주소를 보고 고른다).
// 공개 페이지 밖으로 가는 주소는 전체를 다시 불러 스튜디오 앱이 연다.
import { Suspense, useEffect } from 'react';
import { BrowserRouter, Navigate, Route, Routes, useLocation, useParams } from '../lib/router';
import { ToastProvider } from '../components/Toast';
import { ErrorBoundary } from '../components/ErrorBoundary';
import { lazyComponent } from '../lib/lazyComponent';
import { artistPath, artistPressPath, releasePath } from '../lib/publicLinks';
import { toHref } from '../lib/router';
import '../styles/public.css';

const SmartLink = lazyComponent(() => import('../pages/SmartLink'), m => m.SmartLink);
const ArtistPage = lazyComponent(() => import('../pages/ArtistPage'), m => m.ArtistPage);
const ArtistPress = lazyComponent(() => import('../pages/ArtistPress'), m => m.ArtistPress);
const ArtistNews = lazyComponent(() => import('../pages/ArtistNews'), m => m.ArtistNews);
const ArtistDiscography = lazyComponent(() => import('../pages/ArtistDiscography'), m => m.ArtistDiscography);

/** index.html의 첫 화면과 같은 모양 (스타일은 index.html 안에 있다) */
function Boot() {
  return (
    <div className="aq-boot" role="status" aria-label="불러오는 중" style={{ animation: 'none' }}>
      <img src={`${import.meta.env.BASE_URL}static/AUDENIQ_Logo_Light.svg`} alt="" />
      <span className="aq-boot-bar" />
    </div>
  );
}

/** 예전 공개 주소(/l/·/a/) → 새 주소, 쿼리·해시는 그대로 */
function MovedLink({ to }: { to: (slug: string) => string }) {
  const { slug = '' } = useParams();
  const loc = useLocation();
  return <Navigate to={`${to(slug)}${loc.search}${loc.hash ?? ''}`} replace />;
}

/** 공개 페이지가 아닌 주소 — 스튜디오 앱으로 전체를 다시 불러온다 */
function ToStudio() {
  const loc = useLocation();
  useEffect(() => { window.location.assign(toHref(loc.pathname) + loc.search + (loc.hash ?? '')); }, [loc]);
  return <Boot />;
}

export function PublicApp() {
  return (
    <BrowserRouter>
      <ToastProvider>
        <ErrorBoundary fullPage>
          <Suspense fallback={<Boot />}>
            <Routes>
              <Route path="/link/release/:slug" element={<SmartLink />} />
              <Route path="/link/artist/:slug" element={<ArtistPage />} />
              <Route path="/link/artist/:slug/press" element={<ArtistPress />} />
              <Route path="/link/artist/:slug/news" element={<ArtistNews />} />
              <Route path="/link/artist/:slug/discography" element={<ArtistDiscography />} />
              <Route path="/l/:slug" element={<MovedLink to={releasePath} />} />
              <Route path="/a/:slug" element={<MovedLink to={artistPath} />} />
              <Route path="/a/:slug/press" element={<MovedLink to={artistPressPath} />} />
              <Route path="*" element={<ToStudio />} />
            </Routes>
          </Suspense>
        </ErrorBoundary>
      </ToastProvider>
    </BrowserRouter>
  );
}
