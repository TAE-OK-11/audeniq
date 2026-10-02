-- Add-ons live in the existing catalog; targets always reference existing entities.
ALTER TABLE identity.resources DROP CONSTRAINT resources_kind_check;
ALTER TABLE identity.resources ADD CONSTRAINT resources_kind_check CHECK(kind IN ('artist','label','release','asset','addon_order'));
ALTER TABLE catalog.assets DROP CONSTRAINT assets_kind_check;
ALTER TABLE catalog.assets ADD CONSTRAINT assets_kind_check CHECK(kind IN ('AUDIO','IMAGE','DOCUMENT','VIDEO','LRC'));
ALTER TABLE catalog.upload_sessions DROP CONSTRAINT upload_sessions_expected_bytes_check;
ALTER TABLE catalog.upload_sessions ADD CONSTRAINT upload_sessions_expected_bytes_check CHECK(expected_bytes BETWEEN 1 AND 2147483648);
ALTER TABLE operations.jobs ADD COLUMN addon_priority_previous integer;
ALTER TABLE operations.audit_events ADD COLUMN before_value jsonb;
ALTER TABLE operations.audit_events ADD COLUMN after_value jsonb;

CREATE TABLE catalog.addon_service_catalog (
 id uuid PRIMARY KEY, code text NOT NULL CHECK(code ~ '^[A-Z][A-Z0-9_]{1,63}$'),
 category text NOT NULL, display_name text NOT NULL, description text NOT NULL,
 price_krw bigint NOT NULL CHECK(price_krw>=0), currency text NOT NULL DEFAULT 'KRW' CHECK(currency='KRW'),
 billing_unit text NOT NULL CHECK(billing_unit IN ('artist','release','track','music_video')),
 active boolean NOT NULL DEFAULT true, requires_payment boolean NOT NULL,
 validity_days integer CHECK(validity_days>0), validity_months integer CHECK(validity_months>0), max_revisions integer CHECK(max_revisions>=0),
 version integer NOT NULL CHECK(version>0), created_at timestamptz NOT NULL DEFAULT now(), updated_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE(code,version), CHECK(requires_payment=(price_krw>0))
);
CREATE UNIQUE INDEX addon_catalog_current ON catalog.addon_service_catalog(code) WHERE active;
INSERT INTO catalog.addon_service_catalog(id,code,category,display_name,description,price_krw,billing_unit,requires_payment,validity_days,max_revisions,version) VALUES
 ('a0000000-0000-4000-8000-000000000001','PROFILE_BASIC','ARTIST_PROFILE','Artist Profile Basic','DSP profile linking, YouTube OAC assistance and basic status tracking',0,'artist',false,NULL,NULL,1),
 ('a0000000-0000-4000-8000-000000000002','PROFILE_PLUS','ARTIST_PROFILE','Artist Profile Plus','Profile mismatch, namesake separation, merge, rename and long-term follow-up',19000,'artist',true,365,NULL,1),
 ('a0000000-0000-4000-8000-000000000003','MIGRATION','RELEASE_CARE','Migration','Preserve identifiers and original date; deliver, confirm matching, then request previous takedown',0,'release',false,NULL,NULL,1),
 ('a0000000-0000-4000-8000-000000000004','PRIORITY_DELIVERY','RELEASE_CARE','Priority Delivery','AUDENIQ internal review, QC and delivery queue priority and monitoring; DSP publication time is not promised',10000,'release',true,NULL,NULL,1),
 ('a0000000-0000-4000-8000-000000000005','LYRICS_BASIC','LYRICS','Lyrics Basic','Plain lyrics, manual registration, supplied LRC and basic lyric video request',0,'track',false,NULL,NULL,1),
 ('a0000000-0000-4000-8000-000000000006','AI_SYNC_LYRICS','LYRICS','AI Sync Lyrics','Time-synced lyrics, LRC and basic automatic correction',5000,'track',true,NULL,NULL,1),
 ('a0000000-0000-4000-8000-000000000007','LYRIC_VIDEO_PLUS','LYRICS','Lyric Video Plus','AI Sync Lyrics, premium template render, cover and artist information, high-quality output and two revisions',30000,'track',true,NULL,2,1),
 ('a0000000-0000-4000-8000-000000000008','PROMO_BASIC','PROMOTION','Promotion Basic','Smart link, pre-save, pre-order, QR, DSP links and basic promo card',0,'release',false,NULL,NULL,1),
 ('a0000000-0000-4000-8000-000000000009','MV_REVIEW_AND_GLOBAL','MUSIC_VIDEO','MV Review and Global','AUDENIQ review procedure and global video delivery preparation and tracking',25000,'music_video',true,NULL,NULL,1),
 ('a0000000-0000-4000-8000-000000000010','MV_GLOBAL_ONLY','MUSIC_VIDEO','MV Global Only','Review valid artist-provided evidence before global video delivery preparation',10000,'music_video',true,NULL,NULL,1),
 ('a0000000-0000-4000-8000-000000000011','MUSIC_DATA_BASIC','MUSIC_DATA','Music Data Basic','Supported public music database registration and metadata tracking',0,'release',false,NULL,NULL,1);

UPDATE catalog.addon_service_catalog SET validity_months=12 WHERE code='PROFILE_PLUS';

CREATE TABLE catalog.addon_orders (
 id uuid PRIMARY KEY, org_id uuid NOT NULL, resource_kind text NOT NULL DEFAULT 'addon_order' CHECK(resource_kind='addon_order'),
 requester_user_id uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 service_code text NOT NULL, catalog_version integer NOT NULL,
 price_snapshot_krw bigint NOT NULL CHECK(price_snapshot_krw>=0), amount bigint NOT NULL, currency text NOT NULL CHECK(currency='KRW'),
 validity_days_snapshot integer, validity_months_snapshot integer, max_revisions_snapshot integer,
 payment_status text NOT NULL CHECK(payment_status IN ('NOT_REQUIRED','PENDING','PAID','REFUNDED')),
 payment_reference text, refund_status text CHECK(refund_status IN ('REQUESTED','REFUNDED')), refund_reference text, paid_at timestamptz, refunded_at timestamptz,
 status text NOT NULL CHECK(status IN ('DRAFT','SUBMITTED','PAYMENT_REQUIRED','PAID','QUEUED','UNDER_REVIEW','NEEDS_INFO','APPROVED','IN_PROGRESS','EXTERNAL_PENDING','COMPLETED','REJECTED','CANCELLED','FAILED')),
 target_type text NOT NULL CHECK(target_type IN ('artist','release','track','music_video')), target_id uuid NOT NULL,
 artist_id uuid, release_id uuid, track_id uuid, video_asset_id uuid,
 assigned_admin_user_id uuid REFERENCES identity.users ON DELETE RESTRICT,
 priority integer NOT NULL DEFAULT 0 CHECK(priority IN (0,10,20)), revision_count integer NOT NULL DEFAULT 0 CHECK(revision_count>=0),
 row_version bigint NOT NULL DEFAULT 0, dispatch_generation integer NOT NULL DEFAULT 0, metadata jsonb NOT NULL DEFAULT '{}' CHECK(jsonb_typeof(metadata)='object'),
 submitted_at timestamptz, accepted_at timestamptz, first_reviewed_at timestamptz, processing_at timestamptz,
 completed_at timestamptz, rejected_at timestamptz, cancelled_at timestamptz, valid_until timestamptz,
 created_at timestamptz NOT NULL DEFAULT now(), updated_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE(org_id,id), FOREIGN KEY(org_id,id,resource_kind) REFERENCES identity.resources(org_id,id,kind) ON DELETE RESTRICT,
 FOREIGN KEY(service_code,catalog_version) REFERENCES catalog.addon_service_catalog(code,version) ON DELETE RESTRICT,
 FOREIGN KEY(org_id,artist_id) REFERENCES catalog.artists(org_id,id) ON DELETE RESTRICT,
 FOREIGN KEY(org_id,release_id) REFERENCES catalog.releases(org_id,id) ON DELETE RESTRICT,
 FOREIGN KEY(org_id,track_id) REFERENCES catalog.tracks(org_id,id) ON DELETE RESTRICT,
 FOREIGN KEY(org_id,video_asset_id) REFERENCES catalog.assets(org_id,id) ON DELETE RESTRICT,
 CHECK(amount=price_snapshot_krw),
 CHECK((amount=0 AND payment_status='NOT_REQUIRED') OR (amount>0 AND payment_status<>'NOT_REQUIRED')),
 CHECK(payment_status NOT IN ('PAID','REFUNDED') OR (paid_at IS NOT NULL AND payment_reference IS NOT NULL)),
 CHECK(payment_status<>'REFUNDED' OR (refunded_at IS NOT NULL AND refund_reference IS NOT NULL AND refund_status='REFUNDED')),
 CHECK(status NOT IN ('PAID','QUEUED','UNDER_REVIEW','APPROVED','IN_PROGRESS','EXTERNAL_PENDING','COMPLETED') OR payment_status IN ('NOT_REQUIRED','PAID')),
 CHECK((target_type='artist' AND artist_id IS NOT NULL AND target_id=artist_id AND release_id IS NULL AND track_id IS NULL AND video_asset_id IS NULL)
    OR (target_type='release' AND release_id IS NOT NULL AND target_id=release_id AND artist_id IS NULL AND track_id IS NULL AND video_asset_id IS NULL)
    OR (target_type='track' AND track_id IS NOT NULL AND target_id=track_id AND release_id IS NOT NULL AND video_asset_id IS NULL)
    OR (target_type='music_video' AND video_asset_id IS NOT NULL AND target_id=video_asset_id AND track_id IS NULL AND artist_id IS NULL))
);
CREATE UNIQUE INDEX addon_active_target ON catalog.addon_orders(org_id,service_code,target_id) WHERE status NOT IN ('COMPLETED','REJECTED','CANCELLED');
CREATE INDEX addon_org_page ON catalog.addon_orders(org_id,created_at DESC,id DESC);
CREATE INDEX addon_staff_page ON catalog.addon_orders(status,submitted_at,id);
CREATE INDEX addon_assigned ON catalog.addon_orders(assigned_admin_user_id,status,created_at);
CREATE INDEX addon_service_page ON catalog.addon_orders(service_code,created_at DESC,id DESC);
CREATE INDEX addon_release ON catalog.addon_orders(org_id,release_id);
CREATE INDEX addon_artist ON catalog.addon_orders(org_id,artist_id);
CREATE UNIQUE INDEX addon_payment_reference ON catalog.addon_orders(payment_reference) WHERE payment_reference IS NOT NULL;

CREATE TABLE catalog.addon_idempotency (
 org_id uuid NOT NULL, user_id uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT, key text NOT NULL CHECK(length(key) BETWEEN 8 AND 128),
 request_hash text NOT NULL, order_id uuid NOT NULL, PRIMARY KEY(org_id,user_id,key),
 FOREIGN KEY(org_id,order_id) REFERENCES catalog.addon_orders(org_id,id) ON DELETE RESTRICT
);
CREATE TRIGGER immutable BEFORE UPDATE OR DELETE ON catalog.addon_idempotency FOR EACH ROW EXECUTE FUNCTION operations.reject_mutation();

CREATE TABLE catalog.artist_profile_requests (
 org_id uuid NOT NULL, addon_order_id uuid PRIMARY KEY, request_type text NOT NULL CHECK(request_type IN ('LINK','OAC','MISMATCH','SEPARATE','MERGE','RENAME','RETRY')),
 platforms text[] NOT NULL DEFAULT '{}', notes text NOT NULL DEFAULT '',
 FOREIGN KEY(org_id,addon_order_id) REFERENCES catalog.addon_orders(org_id,id) ON DELETE RESTRICT
);
CREATE TABLE catalog.migration_requests (
 org_id uuid NOT NULL, addon_order_id uuid PRIMARY KEY, previous_distributor text NOT NULL,
 preserve_isrc boolean NOT NULL DEFAULT true CHECK(preserve_isrc), preserve_upc boolean NOT NULL DEFAULT false,
 upc_preservation_status text NOT NULL DEFAULT 'PENDING' CHECK(upc_preservation_status IN ('PENDING','APPROVED','NOT_AVAILABLE')),
 original_release_date date NOT NULL, previous_release_urls text[] NOT NULL,
 delivery_status text NOT NULL DEFAULT 'PENDING' CHECK(delivery_status IN ('PENDING','DELIVERED')),
 matching_status text NOT NULL DEFAULT 'PENDING' CHECK(matching_status IN ('PENDING','CONFIRMED')),
 takedown_status text NOT NULL DEFAULT 'PENDING' CHECK(takedown_status IN ('PENDING','REQUESTED','COMPLETED')),
 CHECK(matching_status<>'CONFIRMED' OR delivery_status='DELIVERED'),
 CHECK(takedown_status='PENDING' OR matching_status='CONFIRMED'),
 FOREIGN KEY(org_id,addon_order_id) REFERENCES catalog.addon_orders(org_id,id) ON DELETE RESTRICT
);
CREATE TABLE catalog.lyrics_requests (
 org_id uuid NOT NULL, addon_order_id uuid PRIMARY KEY, lyrics_text text NOT NULL DEFAULT '', lrc_asset_id uuid,
 basic_video_requested boolean NOT NULL DEFAULT false, sync_status text NOT NULL DEFAULT 'PENDING' CHECK(sync_status IN ('PENDING','READY')),
 FOREIGN KEY(org_id,addon_order_id) REFERENCES catalog.addon_orders(org_id,id) ON DELETE RESTRICT,
 FOREIGN KEY(org_id,lrc_asset_id) REFERENCES catalog.assets(org_id,id) ON DELETE RESTRICT
);
CREATE TABLE catalog.lyric_video_requests (
 org_id uuid NOT NULL, addon_order_id uuid PRIMARY KEY, template_id text NOT NULL, source_asset_id uuid,
 render_status text NOT NULL DEFAULT 'PENDING' CHECK(render_status IN ('PENDING','READY')), output_asset_id uuid,
 FOREIGN KEY(org_id,addon_order_id) REFERENCES catalog.addon_orders(org_id,id) ON DELETE RESTRICT,
 FOREIGN KEY(org_id,source_asset_id) REFERENCES catalog.assets(org_id,id) ON DELETE RESTRICT,
 FOREIGN KEY(org_id,output_asset_id) REFERENCES catalog.assets(org_id,id) ON DELETE RESTRICT
);
CREATE TABLE catalog.mv_requests (
 org_id uuid NOT NULL, addon_order_id uuid PRIMARY KEY,
 review_mode text NOT NULL CHECK(review_mode IN ('AUDENIQ','ARTIST_EVIDENCE')), review_evidence_asset_id uuid,
 evidence_valid_until timestamptz, review_status text NOT NULL DEFAULT 'PENDING' CHECK(review_status IN ('PENDING','APPROVED','REJECTED')),
 review_decided_by uuid REFERENCES identity.users ON DELETE RESTRICT, review_decided_at timestamptz,
 global_distribution_status text NOT NULL DEFAULT 'PENDING' CHECK(global_distribution_status IN ('PENDING','PREPARED','DELIVERED')),
 CHECK(review_mode<>'ARTIST_EVIDENCE' OR review_evidence_asset_id IS NOT NULL),
 CHECK(review_status<>'APPROVED' OR (review_evidence_asset_id IS NOT NULL AND evidence_valid_until IS NOT NULL AND review_decided_by IS NOT NULL)),
 CHECK(global_distribution_status='PENDING' OR review_status='APPROVED'),
 FOREIGN KEY(org_id,addon_order_id) REFERENCES catalog.addon_orders(org_id,id) ON DELETE RESTRICT,
 FOREIGN KEY(org_id,review_evidence_asset_id) REFERENCES catalog.assets(org_id,id) ON DELETE RESTRICT
);
CREATE TABLE catalog.promo_requests (
 org_id uuid NOT NULL, addon_order_id uuid PRIMARY KEY, smartlink_slug text NOT NULL UNIQUE,
 presave_enabled boolean NOT NULL DEFAULT true, preorder_enabled boolean NOT NULL DEFAULT true,
 qr_asset_id uuid, promo_card_asset_id uuid,
 FOREIGN KEY(org_id,addon_order_id) REFERENCES catalog.addon_orders(org_id,id) ON DELETE RESTRICT,
 FOREIGN KEY(org_id,qr_asset_id) REFERENCES catalog.assets(org_id,id) ON DELETE RESTRICT,
 FOREIGN KEY(org_id,promo_card_asset_id) REFERENCES catalog.assets(org_id,id) ON DELETE RESTRICT
);
CREATE TABLE catalog.addon_provider_tasks (
 id uuid PRIMARY KEY, org_id uuid NOT NULL, addon_order_id uuid NOT NULL, job_id uuid NOT NULL REFERENCES operations.jobs ON DELETE RESTRICT,
 kind text NOT NULL, provider text NOT NULL, generation integer NOT NULL, status text NOT NULL DEFAULT 'EXTERNAL_PENDING' CHECK(status IN ('EXTERNAL_PENDING','COMPLETED')),
 external_reference text, created_at timestamptz NOT NULL DEFAULT now(), completed_at timestamptz,
 UNIQUE(job_id,provider), FOREIGN KEY(org_id,addon_order_id) REFERENCES catalog.addon_orders(org_id,id) ON DELETE RESTRICT
);
CREATE TABLE catalog.addon_dsp_links (
 org_id uuid NOT NULL, addon_order_id uuid NOT NULL, platform text NOT NULL, url text NOT NULL,
 PRIMARY KEY(addon_order_id,platform), FOREIGN KEY(org_id,addon_order_id) REFERENCES catalog.addon_orders(org_id,id) ON DELETE RESTRICT
);

-- Orders, details and idempotency remain tenant scoped even under runtime roles.
DO $$ DECLARE t text; BEGIN
 FOREACH t IN ARRAY ARRAY['addon_orders','addon_idempotency','artist_profile_requests','migration_requests','lyrics_requests','lyric_video_requests','mv_requests','promo_requests','addon_provider_tasks','addon_dsp_links'] LOOP
  EXECUTE format('ALTER TABLE catalog.%I ENABLE ROW LEVEL SECURITY',t);
  EXECUTE format('ALTER TABLE catalog.%I FORCE ROW LEVEL SECURITY',t);
  EXECUTE format('CREATE POLICY addon_scope ON catalog.%I USING (org_id=nullif(current_setting(''app.org_id'',true),'''')::uuid OR current_setting(''app.staff'',true)=''on'') WITH CHECK (org_id=nullif(current_setting(''app.org_id'',true),'''')::uuid OR current_setting(''app.staff'',true)=''on'')',t);
 END LOOP;
END $$;

-- Reuse the existing queue priority for both queued and future pipeline jobs.
CREATE TABLE catalog.addon_release_priorities (
 org_id uuid NOT NULL, release_id uuid PRIMARY KEY, addon_order_id uuid NOT NULL, priority integer NOT NULL CHECK(priority IN (10,20)),
 FOREIGN KEY(org_id,release_id) REFERENCES catalog.releases(org_id,id) ON DELETE RESTRICT,
 FOREIGN KEY(org_id,addon_order_id) REFERENCES catalog.addon_orders(org_id,id) ON DELETE RESTRICT
);
CREATE FUNCTION operations.apply_addon_priority() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF NEW.status='QUEUED' AND NEW.queue IN ('qc','rights','distribution','delivery') THEN
  IF EXISTS(SELECT 1 FROM catalog.addon_release_priorities WHERE release_id=NEW.release_id) THEN
   NEW.addon_priority_previous := NEW.priority;
   NEW.priority := greatest(NEW.priority,(SELECT priority FROM catalog.addon_release_priorities WHERE release_id=NEW.release_id));
  END IF;
 END IF;
 RETURN NEW;
END $$;
-- set_job_release_id (0064) runs first: PostgreSQL orders triggers by name.
CREATE TRIGGER zz_addon_priority BEFORE INSERT ON operations.jobs FOR EACH ROW EXECUTE FUNCTION operations.apply_addon_priority();

CREATE FUNCTION catalog.guard_addon_order() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF (NEW.org_id,NEW.requester_user_id,NEW.service_code,NEW.catalog_version,NEW.price_snapshot_krw,NEW.amount,NEW.currency,NEW.validity_days_snapshot,NEW.validity_months_snapshot,NEW.max_revisions_snapshot,NEW.target_type,NEW.target_id,NEW.artist_id,NEW.release_id,NEW.track_id,NEW.video_asset_id)
    IS DISTINCT FROM (OLD.org_id,OLD.requester_user_id,OLD.service_code,OLD.catalog_version,OLD.price_snapshot_krw,OLD.amount,OLD.currency,OLD.validity_days_snapshot,OLD.validity_months_snapshot,OLD.max_revisions_snapshot,OLD.target_type,OLD.target_id,OLD.artist_id,OLD.release_id,OLD.track_id,OLD.video_asset_id) THEN
  RAISE EXCEPTION 'immutable order snapshot or target' USING ERRCODE='23514';
 END IF;
 IF NEW.status<>OLD.status AND NOT EXISTS(SELECT 1 FROM operations.allowed_transitions WHERE axis='addon_order_status' AND old_status=OLD.status AND new_status=NEW.status) THEN
  RAISE EXCEPTION 'forbidden addon transition' USING ERRCODE='23514';
 END IF;
 IF NEW.row_version<>OLD.row_version+1 THEN RAISE EXCEPTION 'addon version must increment' USING ERRCODE='23514'; END IF;
 IF NEW.status<>OLD.status AND NEW.service_code='MV_GLOBAL_ONLY' AND NEW.status IN ('APPROVED','IN_PROGRESS','EXTERNAL_PENDING','COMPLETED') AND NOT EXISTS(
    SELECT 1 FROM catalog.mv_requests m JOIN catalog.assets a ON a.org_id=m.org_id AND a.id=m.review_evidence_asset_id
    JOIN catalog.upload_sessions u ON u.org_id=a.org_id AND u.asset_id=a.id
    WHERE m.org_id=NEW.org_id AND m.addon_order_id=NEW.id AND m.review_status='APPROVED' AND m.evidence_valid_until>clock_timestamp()
      AND a.kind='DOCUMENT' AND a.state='REGISTERED' AND a.sha256 IS NOT NULL AND a.etag IS NOT NULL AND u.status='COMPLETED') THEN
  RAISE EXCEPTION 'MV evidence approval required' USING ERRCODE='23514';
 END IF;
 NEW.updated_at:=now(); RETURN NEW;
END $$;
CREATE TRIGGER addon_order_guard BEFORE UPDATE ON catalog.addon_orders FOR EACH ROW EXECUTE FUNCTION catalog.guard_addon_order();

-- Pricing changes create a new catalog version. Old rows may only be deactivated.
CREATE FUNCTION catalog.guard_addon_catalog() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF TG_OP='DELETE' OR (to_jsonb(NEW)-'active'-'updated_at') IS DISTINCT FROM (to_jsonb(OLD)-'active'-'updated_at') THEN
  RAISE EXCEPTION 'catalog versions are immutable' USING ERRCODE='23514';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER addon_catalog_guard BEFORE UPDATE OR DELETE ON catalog.addon_service_catalog FOR EACH ROW EXECUTE FUNCTION catalog.guard_addon_catalog();
INSERT INTO operations.allowed_transitions VALUES ('addon_order_status','DRAFT','SUBMITTED');
INSERT INTO operations.allowed_transitions VALUES ('addon_order_status','DRAFT','CANCELLED');
INSERT INTO operations.allowed_transitions VALUES ('addon_order_status','SUBMITTED','PAYMENT_REQUIRED');
INSERT INTO operations.allowed_transitions VALUES ('addon_order_status','SUBMITTED','QUEUED');
INSERT INTO operations.allowed_transitions VALUES ('addon_order_status','SUBMITTED','CANCELLED');
INSERT INTO operations.allowed_transitions VALUES ('addon_order_status','PAYMENT_REQUIRED','PAID');
INSERT INTO operations.allowed_transitions VALUES ('addon_order_status','PAYMENT_REQUIRED','CANCELLED');
INSERT INTO operations.allowed_transitions VALUES ('addon_order_status','PAYMENT_REQUIRED','REJECTED');
INSERT INTO operations.allowed_transitions VALUES ('addon_order_status','PAID','QUEUED');
INSERT INTO operations.allowed_transitions VALUES ('addon_order_status','PAID','CANCELLED');
INSERT INTO operations.allowed_transitions VALUES ('addon_order_status','QUEUED','UNDER_REVIEW');
INSERT INTO operations.allowed_transitions VALUES ('addon_order_status','QUEUED','NEEDS_INFO');
INSERT INTO operations.allowed_transitions VALUES ('addon_order_status','QUEUED','CANCELLED');
INSERT INTO operations.allowed_transitions VALUES ('addon_order_status','QUEUED','FAILED');
INSERT INTO operations.allowed_transitions VALUES ('addon_order_status','UNDER_REVIEW','NEEDS_INFO');
INSERT INTO operations.allowed_transitions VALUES ('addon_order_status','UNDER_REVIEW','APPROVED');
INSERT INTO operations.allowed_transitions VALUES ('addon_order_status','UNDER_REVIEW','REJECTED');
INSERT INTO operations.allowed_transitions VALUES ('addon_order_status','UNDER_REVIEW','CANCELLED');
INSERT INTO operations.allowed_transitions VALUES ('addon_order_status','UNDER_REVIEW','FAILED');
INSERT INTO operations.allowed_transitions VALUES ('addon_order_status','NEEDS_INFO','QUEUED');
INSERT INTO operations.allowed_transitions VALUES ('addon_order_status','NEEDS_INFO','REJECTED');
INSERT INTO operations.allowed_transitions VALUES ('addon_order_status','NEEDS_INFO','CANCELLED');
INSERT INTO operations.allowed_transitions VALUES ('addon_order_status','APPROVED','IN_PROGRESS');
INSERT INTO operations.allowed_transitions VALUES ('addon_order_status','APPROVED','NEEDS_INFO');
INSERT INTO operations.allowed_transitions VALUES ('addon_order_status','APPROVED','CANCELLED');
INSERT INTO operations.allowed_transitions VALUES ('addon_order_status','APPROVED','FAILED');
INSERT INTO operations.allowed_transitions VALUES ('addon_order_status','IN_PROGRESS','EXTERNAL_PENDING');
INSERT INTO operations.allowed_transitions VALUES ('addon_order_status','IN_PROGRESS','COMPLETED');
INSERT INTO operations.allowed_transitions VALUES ('addon_order_status','IN_PROGRESS','NEEDS_INFO');
INSERT INTO operations.allowed_transitions VALUES ('addon_order_status','IN_PROGRESS','UNDER_REVIEW');
INSERT INTO operations.allowed_transitions VALUES ('addon_order_status','IN_PROGRESS','CANCELLED');
INSERT INTO operations.allowed_transitions VALUES ('addon_order_status','IN_PROGRESS','FAILED');
INSERT INTO operations.allowed_transitions VALUES ('addon_order_status','EXTERNAL_PENDING','IN_PROGRESS');
INSERT INTO operations.allowed_transitions VALUES ('addon_order_status','EXTERNAL_PENDING','UNDER_REVIEW');
INSERT INTO operations.allowed_transitions VALUES ('addon_order_status','EXTERNAL_PENDING','NEEDS_INFO');
INSERT INTO operations.allowed_transitions VALUES ('addon_order_status','EXTERNAL_PENDING','COMPLETED');
INSERT INTO operations.allowed_transitions VALUES ('addon_order_status','EXTERNAL_PENDING','CANCELLED');
INSERT INTO operations.allowed_transitions VALUES ('addon_order_status','EXTERNAL_PENDING','FAILED');
INSERT INTO operations.allowed_transitions VALUES ('addon_order_status','COMPLETED','UNDER_REVIEW');
INSERT INTO operations.allowed_transitions VALUES ('addon_order_status','FAILED','QUEUED');
INSERT INTO operations.allowed_transitions VALUES ('addon_order_status','FAILED','CANCELLED');
