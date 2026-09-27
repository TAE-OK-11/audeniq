-- 0041: UPC/ISRC issuance.
--
-- Stage 3 issues a UPC for a release without one and an ISRC for each track
-- without one, from the ACTIVE issuer of that kind. Until the company holds a
-- GS1 company prefix and an ISRC registrant code, the active issuers are
-- VIRTUAL: they make test-only codes from ranges no real release uses.
--   UPC  prefix 2  : GS1 restricted-circulation (in-store) number system
--   ISRC prefix XX : ISO 3166 user-assigned code, never a country
-- Execution refuses to send a release with a VIRTUAL code to a real partner.
-- Registering the real ranges (audeniq-admin identifier-issuer register)
-- switches new codes to them; codes already assigned never change.

CREATE TABLE distribution.identifier_issuers (
 id uuid PRIMARY KEY,
 kind text NOT NULL CHECK(kind IN ('UPC','ISRC')),
 mode text NOT NULL CHECK(mode IN ('VIRTUAL','REGISTERED')),
 prefix text NOT NULL,
 active boolean NOT NULL DEFAULT true,
 created_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE(kind, prefix),
 -- UPC-A: company prefix leaves at least one item-reference digit.
 CHECK((kind='UPC' AND prefix ~ '^[0-9]{1,10}$')
    OR (kind='ISRC' AND prefix ~ '^[A-Z]{2}[A-Z0-9]{3}$')),
 -- Virtual ranges are exactly the reserved ones, and a real range is never one.
 CHECK((mode='VIRTUAL') = ((kind='UPC' AND prefix LIKE '2%') OR (kind='ISRC' AND prefix LIKE 'XX%')))
);
CREATE UNIQUE INDEX identifier_issuers_one_active
 ON distribution.identifier_issuers(kind) WHERE active;

-- Last number issued per issuer and scope ('' for UPC, two-digit year for
-- ISRC, whose designation code restarts every year).
CREATE TABLE distribution.identifier_counters (
 issuer_id uuid NOT NULL REFERENCES distribution.identifier_issuers ON DELETE RESTRICT,
 scope text NOT NULL CHECK(scope ~ '^([0-9]{2})?$'),
 last_value bigint NOT NULL CHECK(last_value > 0),
 PRIMARY KEY(issuer_id, scope)
);

INSERT INTO distribution.identifier_issuers(id, kind, mode, prefix) VALUES
 ('7d0b6a52-2f4e-4f53-9a51-0c1b2e3f4a01', 'UPC', 'VIRTUAL', '2'),
 ('7d0b6a52-2f4e-4f53-9a51-0c1b2e3f4a02', 'ISRC', 'VIRTUAL', 'XXAUD');

-- The ledger now also records codes the platform issued.
--   EXISTING: supplied by the artist
--   ISSUED  : issued from a REGISTERED range
--   VIRTUAL : issued from a VIRTUAL range (test only, never delivered)
ALTER TABLE distribution.identifier_assignments
 DROP CONSTRAINT identifier_assignments_source_check;
ALTER TABLE distribution.identifier_assignments
 ADD CONSTRAINT identifier_assignments_source_check
 CHECK(source IN ('EXISTING','ISSUED','VIRTUAL'));
