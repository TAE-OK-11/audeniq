import test from "node:test";
import assert from "node:assert/strict";
import { resolveAssetPath } from "../src/worker.js";

test("audeniq.com serves the landing site", () => {
  assert.equal(resolveAssetPath("audeniq.com", "/"), "/index.html");
  assert.equal(resolveAssetPath("www.audeniq.com", "/anything"), "/index.html");
});

test("studio.audeniq.com serves AUDENIQ STUDIO", () => {
  assert.equal(resolveAssetPath("studio.audeniq.com", "/"), "/studio/index.html");
  assert.equal(resolveAssetPath("studio.audeniq.com", "/catalog"), "/studio/index.html");
});

test("a future domain works without changing the Worker", () => {
  assert.equal(resolveAssetPath("audeniq.kr", "/"), "/index.html");
  assert.equal(resolveAssetPath("studio.audeniq.kr", "/"), "/studio/index.html");
  assert.equal(resolveAssetPath("example-music.com", "/"), "/index.html");
  assert.equal(resolveAssetPath("studio.example-music.com", "/"), "/studio/index.html");
});

test("workers.dev previews both sites by path or query", () => {
  assert.equal(resolveAssetPath("audeniq-web.example.workers.dev", "/"), "/index.html");
  assert.equal(resolveAssetPath("audeniq-web.example.workers.dev", "/studio"), "/studio/index.html");
  assert.equal(resolveAssetPath("localhost", "/", new URLSearchParams("site=studio")), "/studio/index.html");
});

test("shared brand assets bypass the site router", () => {
  assert.equal(resolveAssetPath("audeniq.com", "/assets/AUDENIQ_Logo_Light.svg"), "/assets/AUDENIQ_Logo_Light.svg");
  assert.equal(resolveAssetPath("studio.audeniq.com", "/assets/AUDENIQ_Logo_Light.svg"), "/assets/AUDENIQ_Logo_Light.svg");
});
