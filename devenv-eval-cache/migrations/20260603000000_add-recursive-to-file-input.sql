-- Track whether a directory input must be hashed recursively over its contents.
--
-- Directories observed via `copied source` / tracked devenv paths end up in the
-- Nix store with all of their contents, so a change to any nested file must
-- invalidate the cache. Directories observed via `readDir` only depend on their
-- listing, so they keep the cheaper name-only hashing (recursive = 0).
ALTER TABLE file_input ADD COLUMN recursive BOOLEAN NOT NULL DEFAULT 0;

-- Entries created by older versions do not contain copied-source observations.
-- A cache hit would skip evaluation and never discover those dependencies, so
-- invalidate the old cache once when this tracking mode is introduced.
DELETE FROM eval_input_path;
DELETE FROM eval_env_input;
DELETE FROM eval_resource_spec;
DELETE FROM cached_eval;
