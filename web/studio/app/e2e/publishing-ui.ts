// Run against a demo build (MOCK=true), never a connected account.
// node --experimental-strip-types e2e/publishing-ui.ts
// STUDIO_DIST=/path/to/demo-dist uses that build; STUDIO_URL uses an already running demo.
import assert from 'node:assert/strict';
import { mkdirSync } from 'node:fs';
import { chromium, type Page } from 'playwright';
import { createServer, preview } from 'vite';

const out = 'test-results/publishing-ui';
const routes = [
  ['analysis', '/reports?type=publishing', '.aq-report-card'],
  ['songs', '/releases?tab=works', '.aq-track-row'],
  ['legacy-redirect', '/publishing?tab=royalties', '.settle-entry'],
  ['album', '/releases/r2?tab=publishing', '.aq-album-pub'],
  ['work', '/works/w1', '.aq-pbl-person-read'],
  ['registration', '/works/w2?tab=register', '.aq-fix-panel'],
  ['work-income', '/works/w1?tab=revenue', '.settle-entry'],
  ['work-files', '/works/w1?tab=files', '.aq-pbl-files'],
  ['ar', '/ar', '.aq-ar-card'],
  ['ar-submit', '/ar?work=w4', '#arWork'],
  ['apply', '/publishing/apply?work=w4', '.aq-pbl-song-picker'],
  ['contracts', '/contracts?pub=c1', '.aq-pbl-contract'],
] as const;

async function noOverflow(page: Page, name: string) {
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth > innerWidth);
  if (overflow) {
    await page.screenshot({ path: `${out}/overflow-${name.replace(/[^a-z0-9]+/gi, '-')}.png`, fullPage: true });
    console.log(await page.evaluate(() => [...document.querySelectorAll('*')].filter(e => e.getBoundingClientRect().right > innerWidth + 1).map(e => ({ tag: e.tagName, cls: e.className, width: e.getBoundingClientRect().width, text: e.textContent?.slice(0, 80) })).slice(-15)));
  }
  assert.equal(overflow, false, `${name}: horizontal overflow`);
}

const server = process.env.STUDIO_URL ? null : process.env.STUDIO_DIST
  ? await preview({ build: { outDir: process.env.STUDIO_DIST }, preview: { host: '127.0.0.1', port: 5173, strictPort: true } })
  : await createServer({ server: { host: '127.0.0.1', port: 5173, strictPort: true } });
if (server && 'listen' in server) await server.listen();
const base = process.env.STUDIO_URL ?? 'http://127.0.0.1:5173';
const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || undefined, args: ['--disable-dev-shm-usage'] });
try {
  mkdirSync(out, { recursive: true });
  const context = await browser.newContext({ locale: 'ko-KR', reducedMotion: 'reduce' });
  await context.addInitScript(() => localStorage.setItem('aq.studio.v2.mock.session', JSON.stringify({ email: 'ui-check@example.test' })));
  const page = await context.newPage();
  page.on('dialog', dialog => { void dialog.accept(); });
  const errors: string[] = [];
  page.on('pageerror', e => errors.push(e.message));
  await page.route('**/api/status', route => route.fulfill({ json: { maintenance: [], current: null, upcoming: [] } }));
  for (const width of [320, 390, 768, 1440]) {
    await page.setViewportSize({ width, height: 900 });
    for (const [name, path, ready] of routes) {
      await page.goto(base + path);
      await page.locator(ready).first().waitFor({ state: 'visible' });
      await page.evaluate(() => document.fonts.ready);
      await noOverflow(page, `${width} ${name}`);
      if (width === 390 || width === 1440) await page.screenshot({ path: `${out}/${name}-${width}.png`, fullPage: true });
    }
    console.log(`Layout passed: ${width}px`);
  }
  // Open A&R opens the shared application shell; review links retain entered data.
  await page.goto(base + '/ar');
  await page.getByRole('button', { name: '데모 제출', exact: true }).click();
  await page.locator('#arWork').selectOption('w4');
  await page.locator('#wizardNext').click();
  await page.locator('#arGenre').fill('R&B');
  await page.getByRole('button', { name: '잔잔한', exact: true }).click();
  await page.locator('#wizardNext').click();
  assert.equal(await page.locator('#wizardNext').isDisabled(), true);
  await page.getByRole('checkbox').check();
  assert.equal(await page.locator('#wizardNext').isEnabled(), true);
  await page.getByRole('button', { name: '곡 소개 수정', exact: true }).click();
  assert.equal(await page.locator('#arGenre').inputValue(), 'R&B');
  await page.getByRole('button', { name: '이전으로 돌아가기', exact: true }).click();
  await page.getByRole('button', { name: '이전으로 돌아가기', exact: true }).click();
  await page.locator('#view-ar').waitFor();
  // Album tab: the prep check opens the song sheet at the field to fix, edits show before → after, and saving returns to the list.
  await page.setViewportSize({ width: 390, height: 900 });
  await page.goto(base + '/releases/r2?tab=publishing');
  const prep = page.locator('.aq-album-pub .aq-prep');
  await prep.waitFor();
  assert.match(await prep.locator('.aq-prep-sub').innerText(), /신청 가능|확인 필요|정리 전/);
  await page.locator('.aq-pbl-trackrow', { hasText: '파도' }).click();
  const sheet = page.locator('#modal .aq-pbl-sheet');
  await sheet.waitFor();
  await sheet.getByRole('button', { name: /수정/ }).click();
  assert.equal(await sheet.getByRole('button', { name: '저장' }).count(), 1);
  await sheet.getByLabel('창작 지분', { exact: true }).first().fill('50');
  assert.match(await sheet.locator('.aq-pbl-diff').innerText(), /60% → 50%/);
  await noOverflow(page, 'work sheet edit');
  await page.screenshot({ path: `${out}/album-sheet-390.png` });
  await sheet.getByRole('button', { name: '취소' }).click();
  await page.locator('#modal').getByRole('button', { name: '닫기' }).click();
  await sheet.waitFor({ state: 'detached' });
  // Album tracks open directly; selection controls and the floating action bar are absent.
  assert.equal(await page.locator('.aq-pbl-pick, .aq-pbl-selbar').count(), 0);
  assert.equal(await page.getByRole('button', { name: '전체 선택', exact: true }).count(), 0);
  // Attach materials from a dialog, then preview them inside the song sheet.
  await page.getByRole('button', { name: '자료 첨부' }).first().click();
  await page.locator('#modal #attachWork').waitFor();
  await page.screenshot({ path: `${out}/album-attach-390.png` });
  await page.locator('#modal').getByRole('button', { name: '닫기' }).click();
  await page.locator('#modal #attachWork').waitFor({ state: 'detached' });
  // A song's full page still returns to the same album tab.
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.locator('.aq-pbl-trackrow', { hasText: '파도' }).click();
  await page.locator('#modal').getByRole('link', { name: /곡 전체 정보 보기/ }).click();
  await page.getByRole('button', { name: '앨범으로 돌아가기' }).click();
  await page.waitForURL('**/releases/r2?tab=publishing');
  // Lyrics attached to a song open inside the song page.
  await page.goto(base + '/works/w1?tab=files');
  await page.locator('.aq-pbl-files .ui-row').first().click();
  await page.locator('.aq-pbl-file-text').waitFor();
  // Labels stay visible, optional fields retain values when collapsed, and unfinished splits can continue.
  await page.setViewportSize({ width: 390, height: 900 });
  await page.goto(base + '/works/new?release=r2&track=t4');
  await page.locator('#wizardNext').click();
  const person = page.locator('.aq-pbl-person').first();
  await person.waitFor();
  await person.getByLabel('창작 지분', { exact: true }).fill('100');
  await person.locator('summary').click();
  await person.getByLabel('AUDENIQ 관리 지분').fill('75');
  await person.locator('summary').click();
  await person.locator('summary').click();
  assert.equal(await person.getByLabel('AUDENIQ 관리 지분').inputValue(), '75');
  await person.locator('summary').click();
  await page.locator('#splitCheck').check();
  await page.locator('#wizardNext').click();
  await page.locator('#workIswc').waitFor({ state: 'visible' });
  await page.getByRole('button', { name: '이전으로 돌아가기', exact: true }).click();
  assert.equal(await page.getByLabel('창작 지분', { exact: true }).inputValue(), '100');
  await noOverflow(page, 'split edit');
  await page.screenshot({ path: `${out}/split-390.png`, fullPage: true });
  // Analysis separates estimates; the album filter survives tab navigation and application entry.
  await page.goto(base + '/publishing');
  await page.waitForURL('**/reports?type=publishing');
  await page.locator('.aq-report-card').waitFor();
  assert.match(await page.locator('.aq-report-total').innerText(), /337,860/);
  // Period/condition filter narrows the report and is carried into the royalty list (and its CSV).
  await page.getByLabel('징수 단체').selectOption('JASRAC');
  await page.getByRole('button', { name: /내역 보기/ }).click();
  await page.locator('#pubRoyaltyList').waitFor();
  assert.equal(new URL(page.url()).searchParams.get('src'), 'JASRAC');
  await page.locator('#pubRoyaltyList .settle-entry').first().waitFor();
  assert.equal(await page.locator('#pubRoyaltyList .settle-entry').count(), 1);
  await page.screenshot({ path: `${out}/royalties-filtered-1440.png`, fullPage: true });
  await page.goto(base + '/reports?type=publishing');
  await page.locator('.aq-report-card').waitFor();
  await page.getByLabel('볼 앨범').selectOption('r2');
  assert.equal(new URL(page.url()).searchParams.get('release'), 'r2');
  // Songs live in 발매·곡 관리; the menu no longer has a separate 퍼블리싱 entry.
  await page.goto(base + '/releases?tab=works&release=r2');
  assert.equal(await page.locator('a[href="/publishing"]').count(), 0);
  await page.getByRole('button', { name: '저작권 관리 신청', exact: true }).click();
  await page.waitForURL('**/publishing/apply?release=r2');
  // Staff review lives under 발매 심사 as a sub-tab: claim, then ask for a fix with a document.
  for (const width of [390, 1440]) {
    await page.setViewportSize({ width, height: 900 });
    await page.goto(base + '/admin/publishing');
    await page.locator('.ui-list .ui-row').first().waitFor();
    await noOverflow(page, `${width} admin publishing list`);
    await page.locator('.ui-list .ui-row').first().click();
    await page.locator('.adm-decide').waitFor({ state: 'attached' });
    await noOverflow(page, `${width} admin publishing detail`);
    await page.screenshot({ path: `${out}/admin-publishing-${width}.png`, fullPage: true });
  }
  const side = page.locator('.adm-detail-side');
  await side.getByRole('button', { name: '담당하기' }).click();
  await side.getByRole('button', { name: '보완 요청' }).click();
  await page.locator('#pubReason').fill('김하늘 님 IPI를 적어 주세요.');
  await page.getByRole('button', { name: '공동 작곡 합의서' }).click();
  await page.locator('#modal .adm-form-actions .ui-btn').click();
  await page.locator('.ui-callout.is-success').waitFor();
  await page.locator('.adm-release-hero').getByText('보완 요청', { exact: true }).waitFor();
  assert.deepEqual(errors, []);
  console.log('Publishing UI passed: 12 pages × 4 widths, album prep/sheet/attach, split editing, return navigation, filters, album scope, and report amounts, admin review.');
} finally {
  await browser.close();
  if (server && 'close' in server) await server.close();
  else if (server) await new Promise<void>((resolve, reject) => server.httpServer.close(e => e ? reject(e) : resolve()));
}
