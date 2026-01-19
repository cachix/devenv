-- Drop command caching tables that are no longer used.
-- Eval caching (cached_eval, eval_input_path, eval_env_input) is now the only caching system.

-- Drop old command environment input table
DROP TABLE IF EXISTS env_input;

-- Drop command-to-file junction table
DROP TABLE IF EXISTS cmd_input_path;

-- Drop the command cache table
DROP TABLE IF EXISTS cached_cmd;

-- Clean up orphaned file_input rows that are no longer referenced by any eval
DELETE FROM file_input
WHERE NOT EXISTS (
    SELECT 1
    FROM eval_input_path
    WHERE eval_input_path.file_input_id = file_input.id
);
