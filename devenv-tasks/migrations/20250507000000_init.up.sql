-- Create the task_run table
CREATE TABLE IF NOT EXISTS task_run (
  id INTEGER PRIMARY KEY,
  task_name TEXT NOT NULL UNIQUE,
  last_run INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
  output JSON
);

-- Create the watched_file table
CREATE TABLE IF NOT EXISTS watched_file (
  id INTEGER PRIMARY KEY,
  task_name TEXT NOT NULL,
  path TEXT NOT NULL,
  modified_time INTEGER NOT NULL,
  content_hash TEXT,
  is_directory BOOLEAN NOT NULL DEFAULT 0,
  UNIQUE(task_name, path)
);

-- Create indexes for better performance
CREATE INDEX IF NOT EXISTS idx_watched_file_task ON watched_file(task_name);
CREATE INDEX IF NOT EXISTS idx_watched_file_task_path ON watched_file(task_name, path);