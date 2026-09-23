ALTER TABLE identity.sessions ADD COLUMN id uuid NOT NULL DEFAULT gen_random_uuid();
ALTER TABLE identity.sessions ADD CONSTRAINT sessions_public_id UNIQUE(id);
CREATE INDEX sessions_active_page ON identity.sessions(user_id,id) WHERE revoked_at IS NULL;
