-- Rename table for file inputs
ALTER TABLE file_path
RENAME TO file_input;

ALTER TABLE cmd_input_path
RENAME COLUMN file_path_id TO file_input_id;

CREATE TABLE env_input (
    id INTEGER NOT NULL PRIMARY KEY,
    cached_cmd_id INTEGER NOT NULL,
    name TEXT NOT NULL,
    content_hash CHAR(64) NOT NULL,
    updated_at INTEGER NOT NULL DEFAULT (strftime ('%s', 'now')),
    FOREIGN KEY (cached_cmd_id) REFERENCES cached_cmd (id) ON DELETE CASCADE,
    UNIQUE (cached_cmd_id, name)
);
