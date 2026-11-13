-- migrate:up
INSERT INTO items (name, tags) VALUES ('first', 'seed');
-- migrate:down
DELETE FROM items WHERE name = 'first';
