-- Eval cache for FFI-based evaluation caching
-- Reuses file_input table for file dependencies

CREATE TABLE IF NOT EXISTS cached_eval
(
  id          INTEGER NOT NULL PRIMARY KEY,
  key_hash    CHAR(64) NOT NULL UNIQUE,
  attr_path   TEXT NOT NULL,
  input_hash  CHAR(64) NOT NULL,
  json_output TEXT NOT NULL,
  updated_at  INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_cached_eval_key ON cached_eval(key_hash);

-- Junction table linking eval cache to file inputs (reuses existing file_input table)
CREATE TABLE IF NOT EXISTS eval_input_path
(
  id              INTEGER NOT NULL PRIMARY KEY,
  cached_eval_id  INTEGER NOT NULL,
  file_input_id   INTEGER NOT NULL,
  UNIQUE(cached_eval_id, file_input_id),
  FOREIGN KEY(cached_eval_id)
    REFERENCES cached_eval(id)
    ON UPDATE CASCADE
    ON DELETE CASCADE,
  FOREIGN KEY(file_input_id)
    REFERENCES file_input(id)
    ON UPDATE CASCADE
    ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_eval_input_path_cached_eval_id ON eval_input_path(cached_eval_id);
CREATE INDEX IF NOT EXISTS idx_eval_input_path_file_input_id ON eval_input_path(file_input_id);

-- Environment variable inputs for eval cache
CREATE TABLE IF NOT EXISTS eval_env_input
(
  id              INTEGER NOT NULL PRIMARY KEY,
  cached_eval_id  INTEGER NOT NULL,
  name            TEXT NOT NULL,
  content_hash    CHAR(64) NOT NULL,
  updated_at      INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
  FOREIGN KEY(cached_eval_id)
    REFERENCES cached_eval(id)
    ON DELETE CASCADE,
  UNIQUE(cached_eval_id, name)
);

CREATE INDEX IF NOT EXISTS idx_eval_env_input_cached_eval_id ON eval_env_input(cached_eval_id);
