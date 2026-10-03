-- 0072: 배급 계약서 (AUD-DIST 2.0).
--
-- The release's AGREEMENT document becomes a real distribution agreement:
-- staff enter the commercial terms (exclusivity, AUDENIQ fee, special rates,
-- special terms) before approving, the server renders the agreement body from
-- the release, the party and those terms, and the artist ticks every rights
-- confirmation before signing. Common service rules stay in the terms of
-- service (AUD-TERMS), which the agreement references instead of repeating.
ALTER TABLE portal.documents ADD COLUMN agreement_terms jsonb NULL;
ALTER TABLE portal.documents ADD COLUMN confirmations jsonb NULL;
ALTER TABLE portal.documents ADD CONSTRAINT agreement_terms_shape CHECK (
  agreement_terms IS NULL OR (
    kind = 'AGREEMENT'
    AND agreement_terms->>'form' = 'AUD-DIST 2.0'
    AND (agreement_terms->>'fee_bps')::int BETWEEN 0 AND 10000
    AND agreement_terms->>'exclusivity' IN ('NON_EXCLUSIVE','EXCLUSIVE')
    AND jsonb_typeof(agreement_terms->'required') = 'array'
  )
);
-- A signed 2.0 agreement always carries the artist's confirmations.
ALTER TABLE portal.documents ADD CONSTRAINT agreement_signed_confirmed CHECK (
  agreement_terms IS NULL OR status <> 'SIGNED' OR (
    confirmations IS NOT NULL AND confirmations->>'content_hash' ~ '^[0-9a-f]{64}$'
  )
);
