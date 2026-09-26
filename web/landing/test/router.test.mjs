import test from "node:test";
import assert from "node:assert/strict";
import worker, { resolveAssetPath, studioRedirect } from "../src/worker.js";

test("audeniq.com serves the landing site", () => {
  assert.equal(resolveAssetPath("audeniq.com", "/"), "/index.html");
  assert.equal(resolveAssetPath("audeniq.kr", "/"), "/index.html");
});

test("shared brand assets bypass the site router", () => {
  assert.equal(resolveAssetPath("audeniq.com", "/assets/AUDENIQ_Logo_Light.svg"), "/assets/AUDENIQ_Logo_Light.svg");
  assert.equal(resolveAssetPath("audeniq.com", "/robots.txt"), "/robots.txt");
});

test("old STUDIO entry points go to the React STUDIO", () => {
  assert.equal(studioRedirect("studio.audeniq.com", "/releases/r1"), "https://studio.audeniq.com/releases/r1");
  assert.equal(studioRedirect("audeniq-web.example.workers.dev", "/studio"), "https://studio.audeniq.com/");
  assert.equal(studioRedirect("audeniq-web.example.workers.dev", "/studio/index.html"), "https://studio.audeniq.com/");
  assert.equal(studioRedirect("audeniq-web.example.workers.dev", "/studio/login"), "https://studio.audeniq.com/login");
  assert.equal(studioRedirect("localhost", "/", new URLSearchParams("site=studio")), "https://studio.audeniq.com/");
  assert.equal(studioRedirect("audeniq.com", "/"), null);
  assert.equal(studioRedirect("audeniq.com", "/studios"), null);
});

test("the Worker redirects STUDIO requests with 308", async () => {
  const env = { ASSETS: { fetch: async () => new Response("landing") } };
  const r = await worker.fetch(new Request("https://audeniq.com/studio/notices?page=2"), env);
  assert.equal(r.status, 308);
  assert.equal(r.headers.get("location"), "https://studio.audeniq.com/notices?page=2");
  const home = await worker.fetch(new Request("https://audeniq.com/"), env);
  assert.equal(await home.text(), "landing");
});
