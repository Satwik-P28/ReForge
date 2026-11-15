-- migrate:up
CREATE TABLE items (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL
);
-- migrate:down
DROP TABLE items;
