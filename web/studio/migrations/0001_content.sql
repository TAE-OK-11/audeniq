-- Studio notices and events, served by the edge Worker from D1
-- (binding CONTENT_DB). Written through the Worker's content admin API
-- (Bearer CONTENT_ADMIN_TOKEN) or `wrangler d1 execute`.
CREATE TABLE IF NOT EXISTS notices (
  id TEXT PRIMARY KEY,
  title TEXT NOT NULL CHECK (length(title) BETWEEN 1 AND 200),
  body TEXT NOT NULL CHECK (length(body) <= 20000),
  pinned INTEGER NOT NULL DEFAULT 0 CHECK (pinned IN (0, 1)),
  published_at TEXT NOT NULL,           -- ISO 8601 (UTC); future = scheduled
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
  deleted_at TEXT
);
CREATE INDEX IF NOT EXISTS notices_listing ON notices (deleted_at, pinned DESC, published_at DESC);

CREATE TABLE IF NOT EXISTS events (
  id TEXT PRIMARY KEY,
  title TEXT NOT NULL CHECK (length(title) BETWEEN 1 AND 200),
  summary TEXT NOT NULL DEFAULT '' CHECK (length(summary) <= 300),
  body TEXT NOT NULL CHECK (length(body) <= 20000),
  place TEXT NOT NULL DEFAULT '' CHECK (length(place) <= 120),
  starts_on TEXT NOT NULL,              -- YYYY-MM-DD (KST)
  ends_on TEXT,                         -- YYYY-MM-DD, NULL = one day
  link_url TEXT,
  published_at TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
  deleted_at TEXT,
  CHECK (ends_on IS NULL OR ends_on >= starts_on)
);
CREATE INDEX IF NOT EXISTS events_listing ON events (deleted_at, starts_on DESC);
