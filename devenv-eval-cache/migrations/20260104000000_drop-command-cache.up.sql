-- Drop command caching tables that are no longer used.
-- Eval caching (cached_eval, eval_input_path, eval_env_input) is now the only caching system.

-- Drop old command environment input table
DROP TABLE IF EXISTS env_input;

-- Drop command-to-file junction table
DROP TABLE IF EXISTS cmd_input_path;

-- Drop the command cache table
DROP TABLE IF EXISTS cached_cmd;

-- Note: Orphaned file_input rows are not cleaned up because turso/limbo
-- doesn't support subqueries in WHERE clauses yet.
-- See: https://github.com/tursodatabase/turso/issues/4632
-- This is fine - orphaned rows will be overwritten when new files are tracked.
