-- 0110: cheap change markers for live streams (crates/core/src/live.rs).
--
-- Each open studio tab used to count the org's unread notifications every
-- 3 s (NOT EXISTS over the whole notification history). A stream now reads
-- three index-backed maxima per tick and runs that count only when one of
-- them moved: the newest notification and inquiry change of the org
-- (existing indexes) and the user's newest read (this index).
CREATE INDEX notification_reads_user_read ON portal.notification_reads(user_id, read_at DESC);
