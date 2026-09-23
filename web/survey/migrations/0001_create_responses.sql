-- Private survey answers. No public read endpoint. Keep contact details separate
-- from exported aggregate statistics and restrict database access to operators.
CREATE TABLE IF NOT EXISTS survey_responses (
 id TEXT PRIMARY KEY NOT NULL,
 survey_version INTEGER NOT NULL DEFAULT 2,
 answers_json TEXT NOT NULL,
 beta_preference TEXT NOT NULL DEFAULT '3',
 contact TEXT,
 beta_contact_consent INTEGER NOT NULL DEFAULT 0 CHECK(beta_contact_consent IN (0,1)),
 survey_consent INTEGER NOT NULL DEFAULT 1 CHECK(survey_consent=1),
 created_at TEXT NOT NULL DEFAULT(strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE INDEX IF NOT EXISTS idx_survey_responses_created_at ON survey_responses(created_at);
