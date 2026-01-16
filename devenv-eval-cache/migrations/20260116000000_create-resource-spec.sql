-- Resource specs for cache replay
-- Stores JSON spec per resource type per cached eval
-- Used to replay resource allocations (e.g., ports) on cache hit

CREATE TABLE IF NOT EXISTS eval_resource_spec (
  id              INTEGER NOT NULL PRIMARY KEY,
  cached_eval_id  INTEGER NOT NULL,
  type_id         TEXT NOT NULL,      -- "ports", "tempdirs", etc.
  spec            TEXT NOT NULL,      -- JSON-serialized spec
  updated_at      INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
  FOREIGN KEY(cached_eval_id)
    REFERENCES cached_eval(id)
    ON DELETE CASCADE,
  UNIQUE(cached_eval_id, type_id)
);

CREATE INDEX IF NOT EXISTS idx_eval_resource_spec_cached_eval_id
ON eval_resource_spec(cached_eval_id);
