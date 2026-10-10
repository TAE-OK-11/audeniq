-- 0086: 공지·이벤트·서버 점검·설문 응답 (Cloudflare D1 → Postgres)
--
-- 예전에는 Cloudflare Worker가 D1(audeniq-content, audeniq-survey)에 두고 서빙했다. 화면은 AWS
-- CloudFront + S3, 이 데이터와 API는 백엔드(crate::content, crate::survey)로 옮긴다.
-- 규칙(길이·날짜·종류)은 D1 스키마(web/studio/migrations, web/survey/migrations)와 같다.
-- 삭제는 deleted_at 기록(복구 가능). 설문 응답은 12개월 뒤 지운다(privacy_maintenance::purge_transient, 소유자 권한).
CREATE SCHEMA IF NOT EXISTS content;

CREATE TABLE content.notices (
  id text PRIMARY KEY CHECK (id ~ '^[a-z0-9-]{1,64}$'),
  title text NOT NULL CHECK (char_length(title) BETWEEN 1 AND 200),
  body text NOT NULL CHECK (char_length(body) <= 20000),
  pinned boolean NOT NULL DEFAULT false,
  published_at timestamptz NOT NULL,            -- 미래면 예약
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  deleted_at timestamptz
);
CREATE INDEX notices_listing ON content.notices (pinned DESC, published_at DESC) WHERE deleted_at IS NULL;

CREATE TABLE content.events (
  id text PRIMARY KEY CHECK (id ~ '^[a-z0-9-]{1,64}$'),
  title text NOT NULL CHECK (char_length(title) BETWEEN 1 AND 200),
  summary text NOT NULL DEFAULT '' CHECK (char_length(summary) <= 300),
  body text NOT NULL CHECK (char_length(body) <= 20000),
  place text NOT NULL DEFAULT '' CHECK (char_length(place) <= 120),
  starts_on date NOT NULL,                      -- KST 날짜
  ends_on date,                                 -- NULL = 하루
  link_url text CHECK (link_url IS NULL OR (link_url LIKE 'https://%' AND char_length(link_url) <= 500)),
  published_at timestamptz NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  deleted_at timestamptz,
  CHECK (ends_on IS NULL OR ends_on >= starts_on)
);
CREATE INDEX events_listing ON content.events (starts_on DESC) WHERE deleted_at IS NULL;

CREATE TABLE content.maintenance (
  id text PRIMARY KEY CHECK (id ~ '^[a-z0-9-]{1,64}$'),
  title text NOT NULL CHECK (char_length(title) BETWEEN 1 AND 200),
  body text NOT NULL DEFAULT '' CHECK (char_length(body) <= 2000),
  starts_at timestamptz NOT NULL,
  ends_at timestamptz NOT NULL,                 -- 예상 종료
  kind text NOT NULL DEFAULT 'scheduled' CHECK (kind IN ('scheduled', 'emergency')),
  end_unknown boolean NOT NULL DEFAULT false,   -- 스튜디오에 '종료 시각 미정'
  published_at timestamptz NOT NULL,            -- 이 시각부터 예고가 보인다
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  deleted_at timestamptz,
  CHECK (ends_at > starts_at)
);
CREATE INDEX maintenance_window ON content.maintenance (ends_at) WHERE deleted_at IS NULL;

-- 설문 답변 (비공개, 읽기 API 없음). 연락처는 통계 내보내기와 분리해 운영자만 본다.
CREATE TABLE content.survey_responses (
  id uuid PRIMARY KEY,
  survey_version integer NOT NULL,
  answers jsonb NOT NULL,
  beta_preference text NOT NULL CHECK (beta_preference IN ('', '0', '1', '2', '3')),
  contact text CHECK (contact IS NULL OR char_length(contact) <= 160),
  beta_contact_consent boolean NOT NULL,
  survey_consent boolean NOT NULL CHECK (survey_consent),
  created_at timestamptz NOT NULL DEFAULT now(),
  CHECK (beta_contact_consent = (contact IS NOT NULL))
);
CREATE INDEX survey_responses_created ON content.survey_responses (created_at);

DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'audeniq_api') THEN
    EXECUTE 'GRANT USAGE ON SCHEMA content TO audeniq_api';
    EXECUTE 'GRANT SELECT, INSERT, UPDATE ON content.notices, content.events, content.maintenance TO audeniq_api';
    -- 설문은 접수만: 중복 확인(id)과 추가
    EXECUTE 'GRANT SELECT (id), INSERT ON content.survey_responses TO audeniq_api';
  END IF;
END
$$;
