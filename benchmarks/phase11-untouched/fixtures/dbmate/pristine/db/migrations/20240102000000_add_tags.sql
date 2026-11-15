-- migrate:up
ALTER TABLE items ADD COLUMN tags TEXT;
-- migrate:down
ALTER TABLE items DROP COLUMN tags;
