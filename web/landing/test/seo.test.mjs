import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import worker from "../src/worker.js";

const projectRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (path) => readFileSync(join(projectRoot, path), "utf8");
const env = {
  ASSETS: {
    async fetch(request) {
      const path = new URL(request.url).pathname;
      const filename = join(projectRoot, "public", path);
      try {
        const body = readFileSync(filename);
        const type = path.endsWith(".xml") ? "application/xml" : path.endsWith(".txt") ? "text/plain" : path.endsWith(".svg") ? "image/svg+xml" : path.endsWith(".png") ? "image/png" : "text/html";
        return new Response(body, { status: 200, headers: { "Content-Type": type } });
      } catch {
        return new Response("Not found", { status: 404 });
      }
    }
  }
};
async function fetchPath(host, path = "/") {
  return worker.fetch(new Request(`https://${host}${path}`), env);
}

test("public homepage is indexable only on the primary domain", async () => {
  const live = await fetchPath("audeniq.com");
  assert.equal(live.status, 200);
  assert.equal(live.headers.get("X-Robots-Tag"), null);
  const html = await live.text();
  assert.match(html, /<meta name="robots" content="index, follow, max-image-preview:large">/);
  assert.match(html, /<link rel="canonical" href="https:\/\/audeniq\.com\/">/);
  const preview = await fetchPath("audeniq-web.example.workers.dev");
  assert.match(preview.headers.get("X-Robots-Tag"), /noindex/);
});

test("studio stays out of search on any hostname or preview path", async () => {
  for (const hostPath of [["studio.audeniq.com", "/"], ["audeniq.com", "/studio/"], ["audeniq-web.example.workers.dev", "/?site=studio"]]) {
    const response = await fetchPath(...hostPath);
    assert.equal(response.status, 200);
    assert.match(response.headers.get("X-Robots-Tag"), /noindex/);
    assert.match(await response.text(), /<meta name="robots" content="noindex,nofollow,noarchive">/);
  }
});

test("robots and sitemap are public only on the canonical domain", async () => {
  const robots = await fetchPath("audeniq.com", "/robots.txt");
  assert.equal(robots.status, 200);
  assert.match(await robots.text(), /Sitemap: https:\/\/audeniq\.com\/sitemap\.xml/);
  const sitemap = await fetchPath("audeniq.com", "/sitemap.xml");
  assert.equal(sitemap.status, 200);
  assert.match(sitemap.headers.get("Content-Type"), /xml/);
  assert.match(await sitemap.text(), /<loc>https:\/\/audeniq\.com\/<\/loc>/);
  assert.equal((await fetchPath("studio.audeniq.com", "/robots.txt")).status, 200);
  assert.match(await (await fetchPath("studio.audeniq.com", "/robots.txt")).text(), /Disallow: \/\s*$/);
  assert.match(await (await fetchPath("audeniq-web.example.workers.dev", "/robots.txt")).text(), /Disallow: \/\s*$/);
  assert.equal((await fetchPath("studio.audeniq.com", "/sitemap.xml")).status, 404);
});

test("duplicate home URLs redirect and non-existent landing pages yield real 404", async () => {
  const www = await fetchPath("www.audeniq.com", "/about?a=1");
  assert.equal(www.status, 308);
  assert.equal(www.headers.get("Location"), "https://audeniq.com/about?a=1");
  const index = await fetchPath("audeniq.com", "/index.html");
  assert.equal(index.status, 308);
  assert.equal(index.headers.get("Location"), "https://audeniq.com/");
  const missing = await fetchPath("audeniq.com", "/this-page-does-not-exist");
  assert.equal(missing.status, 404);
  assert.match(missing.headers.get("X-Robots-Tag"), /noindex/);
});

test("social preview is a real 1200x630 PNG and shared assets return their own content", async () => {
  const bytes = readFileSync(join(projectRoot, "public/assets/social-preview.png"));
  assert.deepEqual([...bytes.subarray(0, 8)], [137,80,78,71,13,10,26,10]);
  assert.equal(bytes.readUInt32BE(16), 1200);
  assert.equal(bytes.readUInt32BE(20), 630);
  const og = await fetchPath("audeniq.com", "/assets/social-preview.png");
  assert.equal(og.status, 200);
  assert.match(og.headers.get("Content-Type"), /image\/png/);
  assert.ok(statSync(join(projectRoot, "public/favicon.ico")).size > 0);
});

test("root files and Wrangler public assets are synchronized", () => {
  for (const path of ["index.html", "studio/index.html", "robots.txt", "sitemap.xml", "assets/social-preview.png", "assets/favicon.svg", "favicon.ico"]) {
    assert.deepEqual(readFileSync(join(projectRoot, path)), readFileSync(join(projectRoot, "public", path)), `mismatch: ${path}`);
  }
});

test("structured data is valid JSON with official pages", () => {
  const main = read("index.html");
  const match = main.match(/<script type="application\/ld\+json">([^<]+)<\/script>/);
  assert.ok(match);
  const schema = JSON.parse(match[1]);
  assert.equal(schema["@graph"][0].name, "AUDENIQ");
  assert.equal(schema["@graph"][1].url, "https://audeniq.com/");
  assert.ok(!main.includes('content="assets/social-preview.png"'));
});
