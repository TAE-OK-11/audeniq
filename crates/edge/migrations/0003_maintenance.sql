-- 서버 점검 일정 — 스튜디오가 /api/status로 읽어 예고 배너와 점검 화면을 띄운다.
-- 관리: /admin/content의 '서버 점검' 탭 (/api/content/maintenance, ADMIN 세션 또는 CONTENT_ADMIN_TOKEN)
-- web/studio/migrations의 0002_maintenance + 0003_maintenance_emergency와 같은 최종 모양.
-- 같은 D1을 정적 Studio Worker가 이미 마이그레이션했다면 표가 이미 있으므로 아무것도 바꾸지 않는다.
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
  -- 긴급 점검('긴급 점검 시작')은 emergency, 끝나는 시각을 모르면 end_unknown=1
  kind TEXT NOT NULL DEFAULT 'scheduled' CHECK (kind IN ('scheduled', 'emergency')),
  end_unknown INTEGER NOT NULL DEFAULT 0 CHECK (end_unknown IN (0, 1)),
  CHECK (ends_at > starts_at)
);
CREATE INDEX IF NOT EXISTS maintenance_window ON maintenance (deleted_at, ends_at);
