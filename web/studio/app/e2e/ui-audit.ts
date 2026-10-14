// 화면 점검: 입력칸이 바탕과 같은 색이라 묻히는 곳, 입력칸이 바로 위 요소에 붙은 곳을 찾는다.
// 체험(목) 모드 개발 서버에서: STUDIO_URL=http://localhost:5199 bun e2e/ui-audit.ts
import { chromium, type Page } from 'playwright';

const BASE = (process.env.STUDIO_URL ?? 'http://localhost:5199').replace(/\/$/, '');
const OUT = process.env.AUDIT_OUT ?? 'test-results';

type Finding = { route: string; kind: string; field: string; detail: string };

async function audit(page: Page, route: string): Promise<Finding[]> {
  return page.evaluate((route) => {
    const out: { route: string; kind: string; field: string; detail: string }[] = [];
    const opaque = (c: string) => {
      const m = c.match(/rgba?\(([^)]+)\)/);
      if (!m) return null;
      const p = m[1].split(',').map(x => parseFloat(x));
      if (p.length === 4 && p[3] < 0.9) return null;
      return `${p[0]},${p[1]},${p[2]}`;
    };
    const backdrop = (el: Element): string => {
      let e = el.parentElement;
      while (e) {
        const c = opaque(getComputedStyle(e).backgroundColor);
        if (c) return c;
        e = e.parentElement;
      }
      return '255,255,255';
    };
    const name = (el: Element) => {
      const id = el.id ? `#${el.id}` : '';
      const ph = (el as HTMLInputElement).placeholder ? ` "${(el as HTMLInputElement).placeholder}"` : '';
      return `${el.tagName.toLowerCase()}${id}${ph}`;
    };
    const fields = Array.from(document.querySelectorAll('input:not([type=checkbox]):not([type=radio]):not([type=hidden]):not([type=file]):not([type=range]),select,textarea'));
    for (const f of fields) {
      const r = f.getBoundingClientRect();
      const st = getComputedStyle(f);
      if (r.width < 4 || r.height < 4 || st.visibility === 'hidden' || st.display === 'none') continue;
      const own = opaque(st.backgroundColor);
      const borderless = parseFloat(st.borderTopWidth) === 0 || st.borderTopStyle === 'none';
      if (borderless && (!own || own === backdrop(f)) && st.boxShadow === 'none') {
        out.push({ route, kind: 'blends', field: name(f), detail: `field ${own ?? 'transparent'} on ${backdrop(f)}` });
      }
      // 바로 위 형제(라벨 제외)에 붙은 입력칸
      let prev = f.previousElementSibling;
      while (prev && getComputedStyle(prev).display === 'none') prev = prev.previousElementSibling;
      if (prev && prev.tagName !== 'LABEL' && !prev.matches('label *')) {
        const gap = r.top - prev.getBoundingClientRect().bottom;
        if (gap >= -1 && gap < 4) out.push({ route, kind: 'glued', field: name(f), detail: `${gap.toFixed(1)}px below ${prev.tagName.toLowerCase()}.${(prev as HTMLElement).className}` });
      }
    }
    return out;
  }, route);
}

// 눌러서 고르는 버튼(칩)은 색만 바뀌어야 한다: 누르기 전후 크기를 잰다
async function resizing(page: Page, route: string): Promise<Finding[]> {
  const out: Finding[] = [];
  const chips = page.locator('button[aria-pressed]:visible, button[role=radio]:visible');
  const n = Math.min(await chips.count(), 40);
  for (let i = 0; i < n; i++) {
    const b = chips.nth(i);
    const before = await b.boundingBox().catch(() => null);
    if (!before) continue;
    await b.click({ timeout: 1000 }).catch(() => {});
    await page.waitForTimeout(250);
    const after = await b.boundingBox().catch(() => null);
    const label = ((await b.textContent().catch(() => '')) ?? '').trim().slice(0, 30);
    if (after && (Math.abs(after.width - before.width) > 0.5 || Math.abs(after.height - before.height) > 0.5)) {
      out.push({ route, kind: 'resizes', field: `button "${label}"`, detail: `${before.width.toFixed(0)}x${before.height.toFixed(0)} -> ${after.width.toFixed(0)}x${after.height.toFixed(0)}` });
    }
    await b.click({ timeout: 1000 }).catch(() => {});
    await page.waitForTimeout(150);
  }
  return out;
}

async function main() {
  const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || undefined });
  const page = await browser.newPage({ viewport: { width: 390, height: 844 }, locale: 'ko-KR', deviceScaleFactor: 2 });
  const findings: Finding[] = [];
  await page.goto(`${BASE}/signup`);
  await page.fill('#suName', '점검 아티스트');
  await page.fill('#suEmail', `audit-${Date.now()}@example.test`);
  await page.fill('#suPassword', 'audit-test-password-123');
  await page.fill('#suConfirm', 'audit-test-password-123');
  for (const box of await page.locator('input[type=checkbox]').all()) await box.check().catch(() => {});
  await page.click('button[type=submit]');
  await page.waitForSelector('#view-home', { timeout: 20_000 });
  const routes = (process.env.ROUTES ?? '/,/releases,/upload,/reports,/settlement,/contracts,/rights,/inquiries,/notifications,/events,/notices,/profile').split(',');
  for (const route of routes) {
    await page.goto(`${BASE}${route}`);
    await page.waitForTimeout(1200);
    // 접힌 선택지를 펼쳐서 안쪽 입력칸도 본다
    for (const opt of await page.locator('.aq-option input[type=checkbox]:not(:checked)').all()) await opt.check({ force: true }).catch(() => {});
    for (const chip of await page.locator('button:has-text("기타 (직접 입력)")').all()) await chip.click().catch(() => {});
    await page.waitForTimeout(400);
    findings.push(...await audit(page, route));
    await page.screenshot({ path: `${OUT}/audit${route.replace(/\W+/g, '_') || '_home'}.png`, fullPage: true });
  }
  // 발매 신청 마법사: 임시 저장본의 마지막 단계를 바꿔 각 단계를 연다
  await page.goto(`${BASE}/upload`);
  await page.waitForTimeout(800);
  await page.fill('#f-title', 'Audit Song');
  await page.selectOption('#f-genre', { index: 1 });
  await page.click('button:has-text("다음으로")');
  await page.waitForTimeout(2500);
  for (let step = 1; step <= 5; step++) {
    const id = await page.evaluate((step) => {
      let found = '';
      for (let i = 0; i < localStorage.length; i++) {
        const k = localStorage.key(i)!;
        const v = localStorage.getItem(k)!;
        if (!v.includes('"lastStep"')) continue;
        localStorage.setItem(k, v.replace(/"lastStep":\d+/g, `"lastStep":${step}`));
        const m = v.match(/"id":"([^"]+)"[^{}]*"title":"Audit Song"/) || v.match(/"title":"Audit Song"[^{}]*"id":"([^"]+)"/);
        if (m) found = m[1];
      }
      return found;
    }, step);
    await page.goto(`${BASE}/upload?edit=${id}`);
    await page.waitForTimeout(1500);
    // 선택지를 하나씩 켠다 (켤 때마다 화면이 바뀌므로 매번 다시 찾는다)
    for (let n = 0; n < 20; n++) {
      const off = page.locator('label.aq-option:not(.is-on):not(.is-unavailable)').first();
      if (!(await off.count())) break;
      await off.click().catch(() => {});
      await page.waitForTimeout(150);
    }
    for (const t of await page.locator('.cover-track-item input[type=checkbox]:not(:checked)').all()) await t.check({ force: true }).catch(() => {});
    for (const chip of await page.locator('button:has-text("기타 (직접 입력)")').all()) await chip.click().catch(() => {});
    await page.waitForTimeout(500);
    findings.push(...await audit(page, `/upload#step${step + 1}`));
    findings.push(...await resizing(page, `/upload#step${step + 1}`));
    await page.screenshot({ path: `${OUT}/audit_upload_step${step + 1}.png`, fullPage: true });
  }
  await browser.close();
  for (const f of findings) console.log(`${f.kind}\t${f.route}\t${f.field}\t${f.detail}`);
  console.log(`${findings.length} finding(s)`);
}

main().catch(e => { console.error(e); process.exit(1); });
