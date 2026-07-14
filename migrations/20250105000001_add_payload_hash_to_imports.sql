-- Add payload_hash column for idempotency payload mismatch detection.
-- Nullable because existing imports will not have a hash.

ALTER TABLE imports.imports ADD COLUMN payload_hash TEXT;
