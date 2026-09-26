-- 서버 점검 일정 — 스튜디오가 /api/status로 읽어 예고 배너와 점검 화면을 띄운다.
-- 관리: /content-admin의 '서버 점검' 탭 (/api/content/maintenance, CONTENT_ADMIN_TOKEN)
CREATE TABLE IF NOT EXISTS maintenance (
  id TEXT PRIMARY KEY,
  title TEXT NOT NULL CHECK (length(title) BETWEEN 1 AND 200),
  body TEXT NOT NULL DEFAULT '' CHECK (length(body) <= 2000),
  starts_at TEXT NOT NULL,              -- ISO 8601 (UTC)
  ends_at TEXT NOT NULL,                -- ISO 8601 (UTC), 예상 종료
  published_at TEXT NOT NULL,           -- 이 시각부터 예고가 보인다
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
  deleted_at TEXT,
  CHECK (ends_at > starts_at)
);
CREATE INDEX IF NOT EXISTS maintenance_window ON maintenance (deleted_at, ends_at);
