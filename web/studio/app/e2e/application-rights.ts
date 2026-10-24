import assert from 'node:assert/strict';
import { mkdirSync } from 'node:fs';
import { chromium, type Page } from 'playwright';

const base = process.env.STUDIO_URL || 'http://127.0.0.1:5190';
const release = {
  id: 'rights-test', title: '전자 문서 테스트', artist: 'Test Artist', status: 'draft', release_date: '2026-12-01', created_at: '2026-10-01', track_count: 1, tracks: [],
  draft: { artist: 'Test Artist', type: 'single', language: 'ko', genre: 'Pop', label: '', notes: '', upc: '', coverName: '', originalDate: '', territories: ['WORLD'], platforms: ['spotify'], ownership: 'Test Owner', phonogram: '2026 Test Owner', copyright: '2026 Test Owner', rightsChecks: {}, history: [], lastStep: 4,
    draftTracks: [{ id: 'track-test', title: 'Test Track', version: '', isrc: '', composers: 'Writer', lyricists: 'Writer', arrangers: '', performers: '', producer: '', lyrics: '', audioName: 'test.wav', audioSize: 100, explicit: false, duration: '03:00' }],
  },
};
const browser = await chromium.launch({ args: ['--no-sandbox'] });
const errors: string[] = [];
mkdirSync('test-results', { recursive: true });
async function squareClose(page: Page) {
  const close = page.getByRole('button', { name: '닫기', exact: true });
  await close.hover();
  const shape = await close.evaluate(e => { const r = e.getBoundingClientRect(); return { w: r.width, h: r.height, transform: getComputedStyle(e).transform }; });
  assert.equal(shape.w, 40); assert.equal(shape.h, 40); assert.equal(shape.transform, 'none');
}
async function focusStyle(page: Page, selector: string) {
  const idle = await page.locator(selector).evaluate(e => getComputedStyle(e).backgroundColor);
  await page.locator(selector).focus();
  await page.waitForTimeout(220);
  return page.locator(selector).evaluate(e => { const s = getComputedStyle(e); return { background: s.backgroundColor, shadow: s.boxShadow, transition: s.transition }; }).then(f => ({ idle, ...f }));
}
try {
  for (const width of [1280, 390]) {
    const context = await browser.newContext({ viewport: { width, height: 900 }, locale: 'ko-KR' });
    await context.addInitScript(data => {
      if (localStorage.getItem('rights-fixture')) return;
      localStorage.setItem('aq.studio.v2.mock.session', JSON.stringify({ email: 'rights@example.test' }));
      localStorage.setItem('aq.studio.v2.mock.releases', JSON.stringify([data]));
      localStorage.setItem('aq.studio.v2.docs', '[]'); localStorage.setItem('rights-fixture', '1');
    }, release);
    const page = await context.newPage();
    page.on('pageerror', e => errors.push(e.message));
    await page.route('**/api/status', r => r.fulfill({ json: { now: new Date().toISOString(), maintenance: { active: null, upcoming: null } } }));
    await page.goto(`${base}/upload?edit=rights-test`);
    await page.locator('#f-ownership').waitFor();
    const ownerStyle = await focusStyle(page, '#f-ownership');
    // 포커스: 윤곽선·그림자 없이 면 색만 한 단계 진해진다 (페이지 위 회색 칸 → 더 진한 회색, 카드 위 흰 칸 → 회색)
    assert.equal(ownerStyle.shadow, 'none'); assert.equal(ownerStyle.background, 'rgb(232, 237, 245)'); assert.notEqual(ownerStyle.idle, ownerStyle.background);
    if (width === 1280) {
      await page.locator('.aq-wiz-steps').getByRole('button', { name: '트랙 등록', exact: true }).click();
      await page.locator('.track-detail-toggle').click();
      const isrc = await focusStyle(page, '#tr-0-isrc');
      assert.equal(isrc.shadow, 'none'); assert.notEqual(isrc.idle, isrc.background); assert.equal(isrc.transition, ownerStyle.transition);
      await page.locator('.aq-wiz-steps').getByRole('button', { name: '권리 확인', exact: true }).click();
    }
    // 특별한 경우 목록은 '있어요'를 고른 뒤에 열린다
    await page.getByRole('radiogroup', { name: '특별한 경우가 있나요?' }).getByRole('radio', { name: '있어요' }).click();
    await page.getByRole('checkbox', { name: '샘플링·타인 음원 사용', exact: true }).check();
    await page.waitForFunction(() => {
      const selected = document.querySelector('.aq-option.is-on');
      return selected && getComputedStyle(selected).backgroundColor === 'rgb(59, 99, 243)';
    });
    const selected = await page.locator('.aq-option.is-on').first().evaluate(e => ({ color: getComputedStyle(e).backgroundColor, outline: getComputedStyle(e).outlineStyle }));
    assert.equal(selected.color, 'rgb(59, 99, 243)'); assert.equal(selected.outline, 'none');
    await page.getByRole('button', { name: /허락서 작성하기|권리자 추가로 서명받기/ }).first().click();
    await page.locator('#aqDocRightsHolder').waitFor(); await squareClose(page);
    await page.fill('#aqDocRightsHolder', '원본 권리자'); await page.fill('#aqDocSource', 'Original Track / 00:10–00:20 샘플');
    await page.getByRole('button', { name: '자동 작성된 문서 확인하기' }).click();
    // 서명은 권리자 본인이 서명 링크(/sign/…)에서 본인확인 후 한다 — 여기서는 링크가 만들어지는 데까지
    assert.match(await page.locator('.aq-document-snapshot').innerText(), /원본 권리자/);
    await page.getByRole('button', { name: '서명 링크 만들기', exact: true }).click();
    assert.match(await page.locator('.aq-sign-link code').innerText(), /\/sign\//);
    await page.getByRole('button', { name: '확인', exact: true }).click();
    const overflow = await page.evaluate(() => document.documentElement.scrollWidth - innerWidth);
    assert.ok(overflow <= 1, `width ${width} overflow ${overflow}`);
    await page.screenshot({ path: `test-results/rights-document-${width}.png`, fullPage: true });
    if (width === 1280) {
      await page.goto(`${base}/upload?edit=rights-test`);
      await page.getByRole('checkbox', { name: '기존 발매 이전·재발매', exact: true }).check();
      await page.getByRole('button', { name: '유통사를 AUDENIQ으로 이전', exact: true }).click();
      await page.selectOption('#aqRereleaseAudio', 'same'); await page.fill('#aqPreviousIsrc-0', 'KR-ABC-26-00001');
      await page.getByRole('button', { name: '기존 ISRC를 트랙 등록에 적용', exact: true }).click();
      await page.locator('.aq-wiz-steps').getByRole('button', { name: '트랙 등록', exact: true }).click();
      await page.locator('.track-detail-toggle').click(); assert.equal(await page.inputValue('#tr-0-isrc'), 'KR-ABC-26-00001');
      const content = { notices: [] as any[], events: [] as any[] };
      await page.route('**/api/content/**', async r => {
        const req = r.request(), kind = new URL(req.url()).pathname.split('/')[3] as 'notices' | 'events';
        if (req.method() === 'GET') return r.fulfill({ json: { items: content[kind] || [], now: new Date().toISOString() } });
        assert.equal(req.method(), 'POST'); assert.ok(!req.headers().authorization);
        const row = { ...req.postDataJSON(), id: `${kind}-new`, created_at: new Date().toISOString(), updated_at: new Date().toISOString(), deleted_at: null };
        content[kind].push(row); return r.fulfill({ status: 201, json: row });
      });
      await page.goto(`${base}/content-admin`); await page.waitForURL('**/admin/content');
      assert.equal(await page.locator('#cadminToken').count(), 0);
      await page.getByRole('button', { name: '새 공지 쓰기' }).click(); await page.fill('#caTitle', '관리자 공지'); await page.fill('#caBody', '직접 작성한 본문'); await page.getByRole('button', { name: '게시하기', exact: true }).click();
      await page.locator('.aq-cadmin-row-title').filter({ hasText: '관리자 공지' }).waitFor(); assert.equal(content.notices.length, 1);
      await page.getByRole('tab', { name: '이벤트', exact: true }).click(); await page.getByRole('button', { name: '새 이벤트 쓰기' }).click(); await page.fill('#caTitle', '관리자 이벤트'); await page.getByRole('button', { name: '게시하기', exact: true }).click();
      await page.locator('.aq-cadmin-row-title').filter({ hasText: '관리자 이벤트' }).waitFor(); assert.equal(content.events.length, 1);
      await page.evaluate(() => scrollTo(0, 0));
      await page.screenshot({ path: 'test-results/admin-content.png', fullPage: true });
    }
    await context.close(); console.log(`OK application rights and document persistence at ${width}px`);
  }
  assert.deepEqual(errors, []); console.log('OK focus consistency, selection fill, close icon, signing, integrity, ISRC mapping, admin notice/event posting');
} finally { await browser.close(); }
