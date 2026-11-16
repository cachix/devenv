#!/usr/bin/env bash
set -euo pipefail

printf 'keep me' > 'conflicting file'
mkdir 'conflicting directory'
printf 'keep me too' > 'conflicting directory/content'

check_links() {
  test -L 'linked directory'
  test -L 'linked file'
  test "$(cat 'linked directory/content')" = "source-$1"
  test "$(cat 'linked file')" = "version $2"
  test "$(cat 'conflicting file')" = 'keep me'
  test "$(cat 'conflicting directory/content')" = 'keep me too'
  test ! -L 'conflicting file'
  test ! -L 'conflicting directory'
  # No link may have been created inside either immutable source directory.
  test "$(ls -A "$(readlink 'linked directory')")" = content
  test "$(cat source-one/content)" = source-one
  test "$(cat source-two/content)" = source-two
  test "$(ls -A source-one)" = content
  test "$(ls -A source-two)" = content
  jq -e '.managedFiles | sort == ["linked directory", "linked file"]' .devenv/state/files.json
}

# First creation and unchanged reentry through the generated task.
devenv tasks run devenv:files
check_links one 1
old_directory=$(readlink 'linked directory')
old_file=$(readlink 'linked file')
devenv tasks run devenv:files
check_links one 1
test "$(readlink 'linked directory')" = "$old_directory"
test "$(readlink 'linked file')" = "$old_file"

# Changed directory and file targets during actual repeated shell entry.
printf '2\n' > version.nix
devenv shell -- true
check_links two 2
test "$(readlink 'linked directory')" != "$old_directory"
test "$(readlink 'linked file')" != "$old_file"
test "$(ls -A "$old_directory")" = content
devenv shell -- true
check_links two 2

# Broken symlinks are replaced as links, too.
rm 'linked directory' 'linked file'
ln -s "$PWD/missing directory" 'linked directory'
ln -s "$PWD/missing file" 'linked file'
devenv tasks run devenv:files
check_links two 2

# Existing orphan cleanup behavior, using names without spaces: cleanup's
# whitespace handling is a separate concern from symlink replacement.
ln -s "$old_file" orphan
jq '.managedFiles += ["orphan"]' .devenv/state/files.json > state.tmp
mv state.tmp .devenv/state/files.json
devenv tasks run devenv:files
test ! -L orphan
check_links two 2
