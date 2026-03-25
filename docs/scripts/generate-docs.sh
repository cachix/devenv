#!/usr/bin/env bash
set -ex

result=$(devenv-build outputs.devenv-generated-docs)

# Copy the generated docs (can't symlink — Nix store is read-only)
target="$DEVENV_ROOT/docs/src/_generated"
chmod -R u+w "$target" 2>/dev/null || true
rm -rf "$target"
cp -rL "$result" "$target"
chmod -R u+w "$target"

# Create stub doc files for any modules that don't have one yet
for dir in languages services supported-process-managers; do
  for stub in "$result"/stubs/"$dir"/*.md; do
    [ -f "$stub" ] || continue
    name=$(basename "$stub")
    dest="$DEVENV_ROOT/docs/src/$dir/$name"
    if [ ! -f "$dest" ]; then
      cp "$stub" "$dest"
      echo "Created missing doc file: $dest"
    fi
  done
done
