CREATE TABLE IF NOT EXISTS cached_cmd
(
  id             INTEGER NOT NULL PRIMARY KEY,
  raw            TEXT NOT NULL,
  cmd_hash       CHAR(64) NOT NULL UNIQUE,
  input_hash     CHAR(64) NOT NULL,
  output         TEXT NOT NULL,
  updated_at     INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_cached_cmd_hash ON cached_cmd(cmd_hash);

CREATE TABLE IF NOT EXISTS file_path
(
  id           INTEGER NOT NULL PRIMARY KEY,
  path         BLOB NOT NULL UNIQUE,
  is_directory BOOLEAN NOT NULL,
  content_hash CHAR(64) NOT NULL,
  modified_at  INTEGER NOT NULL,
  updated_at   INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_file_path ON file_path(path);

CREATE TABLE IF NOT EXISTS cmd_input_path
(
  id                     INTEGER NOT NULL PRIMARY KEY,
  cached_cmd_id          INTEGER,
  file_path_id           INTEGER,
  UNIQUE(cached_cmd_id, file_path_id),
  FOREIGN KEY(cached_cmd_id)
    REFERENCES cached_cmd(id)
    ON UPDATE CASCADE
    ON DELETE CASCADE,
  FOREIGN KEY(file_path_id)
    REFERENCES file_path(id)
    ON UPDATE CASCADE
    ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_cmd_input_path_cached_cmd_id ON cmd_input_path(cached_cmd_id);
CREATE INDEX IF NOT EXISTS idx_cmd_input_path_file_path_id ON cmd_input_path(file_path_id);
CREATE INDEX IF NOT EXISTS idx_cmd_input_path_composite ON cmd_input_path(cached_cmd_id, file_path_id);
