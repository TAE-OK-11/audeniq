-- F4 Stage 3: release identifiers + artwork linkage for DDEX ERN.
-- UPC lives on the release row (catalog-owned); the identifier assignment
-- ledger itself stays in Astra's 0011+ scope.

ALTER TABLE catalog.releases
    ADD COLUMN upc text CHECK (upc IS NULL OR upc ~ '^[0-9]{12,14}$'),
    ADD COLUMN artwork_asset_id uuid,
    ADD CONSTRAINT releases_artwork_asset_fk
        FOREIGN KEY (org_id, artwork_asset_id) REFERENCES catalog.assets (org_id, id) ON DELETE RESTRICT;
