-- Track whether a directory input must be hashed recursively over its contents.
--
-- Directories observed via `copied source` / tracked devenv paths end up in the
-- Nix store with all of their contents, so a change to any nested file must
-- invalidate the cache. Directories observed via `readDir` only depend on their
-- listing, so they keep the cheaper name-only hashing (recursive = 0).
ALTER TABLE file_input ADD COLUMN recursive BOOLEAN NOT NULL DEFAULT 0;
