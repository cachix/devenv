-- Rename attr_path column to attr_name for consistency with API naming.
-- In SQLite, we need to recreate the table to rename a column.

-- Create new table with renamed column
CREATE TABLE cached_eval_new
(
  id          INTEGER NOT NULL PRIMARY KEY,
  key_hash    CHAR(64) NOT NULL UNIQUE,
  attr_name   TEXT NOT NULL,
  input_hash  CHAR(64) NOT NULL,
  json_output TEXT NOT NULL,
  updated_at  INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);

-- Copy data from old table
INSERT INTO cached_eval_new (id, key_hash, attr_name, input_hash, json_output, updated_at)
SELECT id, key_hash, attr_path, input_hash, json_output, updated_at
FROM cached_eval;

-- Drop old table
DROP TABLE cached_eval;

-- Rename new table
ALTER TABLE cached_eval_new RENAME TO cached_eval;

-- Recreate index
CREATE INDEX IF NOT EXISTS idx_cached_eval_key ON cached_eval(key_hash);
