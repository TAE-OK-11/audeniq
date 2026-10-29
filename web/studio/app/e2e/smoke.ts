// 실서버 연결 빌드(React)를 진짜 API에 붙여 도는 브라우저 스모크 테스트.
// 실행: API(APP_ORIGIN=http://localhost:5173)를 띄우고 `bun run preview:api` 후 `bun run e2e`
//   STUDIO_URL (기본 http://localhost:5173), CHROME_PATH (설치된 Chromium을 쓸 때)
import { chromium, type Page } from 'playwright';
import { mkdirSync } from 'node:fs';

const BASE = (process.env.STUDIO_URL ?? 'http://localhost:5173').replace(/\/$/, '');
const OUT = 'test-results';

async function step(name: string, page: Page, fn: () => Promise<void>) {
  try {
    await fn();
    console.log(`OK   ${name}`);
  } catch (e) {
    mkdirSync(OUT, { recursive: true });
    await page.screenshot({ path: `${OUT}/fail-${name.replace(/\W+/g, '_')}.png`, fullPage: true }).catch(() => {});
    throw new Error(`${name}: ${(e as Error).message.split('\n')[0]}`);
  }
}

async function main() {
  const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || undefined });
  const page = await browser.newPage({ viewport: { width: 1280, height: 900 }, locale: 'ko-KR' });
  const errors: string[] = [];
  page.on('pageerror', e => errors.push(e.message));
  const email = `smoke-${Date.now()}@example.test`;
  const password = 'smoke-test-password-123';
  const title = `Smoke Release ${Date.now().toString(36)}`;

  try {
    await step('signup creates a session', page, async () => {
      // 1단계 이메일·비밀번호 → 2단계 활동명·휴대폰·약관 (가입 직후 문자 인증 화면은 문자 서비스가 없으면 건너뛴다)
      await page.goto(`${BASE}/signup`);
      await page.fill('#suEmail', email);
      await page.fill('#suPassword', password);
      await page.fill('#suConfirm', password);
      await page.click('form button[type=submit]');
      await page.fill('#suName', '스모크 아티스트');
      await page.fill('#suPhone', '010-1234-5678');
      await page.check('#suTerms');
      await page.click('form button[type=submit]');
      // 패스키를 쓸 수 있는 브라우저면 마지막 단계(패스키 만들기)가 나온다 — 나중에 하기
      const later = page.getByRole('button', { name: '나중에 할게요' });
      await Promise.race([page.waitForSelector('#view-home', { timeout: 20_000 }), later.waitFor({ timeout: 20_000 })]);
      if (await later.isVisible()) await later.click();
      await page.waitForSelector('#view-home', { timeout: 20_000 });
    });

    await step('profile saves and survives a reload (session + CSRF recovery)', page, async () => {
      await page.goto(`${BASE}/profile`);
      await page.click('#profileRowName');
      await page.fill('#profileName', '스모크 <safe>');
      await page.click('#profileForm button[type=submit]');
      await page.waitForTimeout(1200);
      await page.reload();
      await page.waitForFunction(() => document.querySelector('#profileRowName')?.textContent?.includes('스모크 <safe>'), null, { timeout: 15_000 });
    });

    await step('release draft is saved on the server', page, async () => {
      await page.goto(`${BASE}/upload`);
      await page.fill('#f-artist', 'Smoke Artist');
      await page.fill('#f-title', title);
      // 자동 임시 저장 후 목록에 보인다
      await page.waitForTimeout(3000);
      await page.goto(`${BASE}/releases`);
      await page.waitForSelector(`text=${title}`, { timeout: 20_000 });
    });

    await step('unknown path shows the 404 card', page, async () => {
      await page.goto(`${BASE}/no/such/page`);
      await page.waitForSelector('.aq-errscreen.is-not-found', { timeout: 10_000 });
    });

    mkdirSync(OUT, { recursive: true });
    await page.goto(`${BASE}/`);
    await page.waitForSelector('#view-home');
    await page.screenshot({ path: `${OUT}/studio.png` });

    await step('logout returns to login', page, async () => {
      // 넓은 화면은 왼쪽 고정 메뉴라 햄버거 메뉴가 없다 — 어느 폭에서나 있는 내 정보 화면에서 로그아웃
      await page.goto(`${BASE}/profile`);
      await page.getByRole('button', { name: '로그아웃' }).first().click();
      await page.getByRole('dialog').getByRole('button', { name: '로그아웃' }).click();
      await page.waitForURL(u => u.pathname.startsWith('/login'), { timeout: 10_000 });
    });

    if (errors.length) throw new Error(`page errors: ${errors.join(' | ')}`);
    console.log('Studio smoke passed: signup, session reload, profile and release draft writes, 404, logout');
  } finally {
    await browser.close();
  }
}

main().catch(e => {
  console.error(`FAIL ${(e as Error).message}`);
  process.exit(1);
});
