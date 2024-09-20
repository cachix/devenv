CREATE TABLE IF NOT EXISTS nix_command
(
  id             INTEGER NOT NULL PRIMARY KEY,
  raw            TEXT NOT NULL,
  command_hash   CHAR(64) NOT NULL UNIQUE,
  output         TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_nix_command_command_hash ON nix_command(command_hash);

CREATE TABLE IF NOT EXISTS file
(
  id           INTEGER NOT NULL PRIMARY KEY,
  path         BLOB NOT NULL UNIQUE,
  content_hash CHAR(64) NOT NULL,
  updated_at   INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_file_path ON file(path);

CREATE TABLE IF NOT EXISTS input_file
(
  id             INTEGER NOT NULL PRIMARY KEY,
  nix_command_id INTEGER,
  file_id        INTEGER,
  UNIQUE(nix_command_id, file_id),
  FOREIGN KEY(nix_command_id)
    REFERENCES nix_command(id)
    ON UPDATE CASCADE
    ON DELETE CASCADE,
  FOREIGN KEY(file_id)
    REFERENCES file(id)
    ON UPDATE CASCADE
    ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_input_file_nix_command_id ON input_file(nix_command_id);
CREATE INDEX IF NOT EXISTS idx_input_file_file_id ON input_file(file_id);
CREATE INDEX IF NOT EXISTS idx_input_file_composite ON input_file(nix_command_id, file_id);
