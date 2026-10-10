-- Signed, artist-authored rights permissions. Signing completes the document;
-- staff approval remains a separate REVIEW -> APPROVED/NEEDS transition.
ALTER TABLE portal.documents ADD COLUMN electronic_record jsonb NULL;
ALTER TABLE portal.documents ADD CONSTRAINT electronic_rights_signed CHECK (
  electronic_record IS NULL OR (
    kind = 'RIGHTS_PROOF' AND signed_at IS NOT NULL AND signed_by IS NOT NULL
    AND signature <> '' AND checked_at IS NOT NULL
    AND electronic_record->>'form' = 'AUD-RIGHTS 1.0'
    AND electronic_record->>'content_hash' ~ '^[0-9a-f]{64}$'
    AND electronic_record->>'document_no' IS NOT NULL
  )
);
CREATE UNIQUE INDEX documents_electronic_receipt
  ON portal.documents (org_id, (electronic_record->>'document_no'))
  WHERE electronic_record IS NOT NULL;
