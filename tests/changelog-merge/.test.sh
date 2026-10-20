#!/usr/bin/env bash
set -euo pipefail

changelog-merge --version
test "$(git config --get test.preserved)" = "existing value"
test "$(git config --get merge.changelog-merge.name)" = "Keep a Changelog semantic merger"
test "$(git config --get merge.changelog-merge.recursive)" = binary
test -n "$(git config --get merge.changelog-merge.driver)"

repo=$(mktemp -d)
trap 'rm -rf "$repo"' EXIT
cd "$repo"
git init -b main
git config user.name Test
git config user.email test@example.com
git config commit.gpgsign false
printf 'CHANGELOG.md merge=changelog-merge\n' > .gitattributes
cat > CHANGELOG.md <<'EOF'
# Changelog

## [Unreleased]

### Added

- Initial feature.
EOF
git add .
git commit -m base
base=$(git rev-parse HEAD)
cp .git/config config-before

git checkout -b other
sed 's/- Initial feature./- Added SSH support.\n- Initial feature./' CHANGELOG.md > changelog.tmp
mv changelog.tmp CHANGELOG.md
git commit -am 'Add SSH'
git checkout main
sed 's/- Initial feature./- Added S3 support.\n- Initial feature./' CHANGELOG.md > changelog.tmp
mv changelog.tmp CHANGELOG.md
git commit -am 'Add S3'
git merge --no-edit other
grep -Fx -- '- Added S3 support.' CHANGELOG.md
grep -Fx -- '- Added SSH support.' CHANGELOG.md
grep -Fx -- '- Initial feature.' CHANGELOG.md
! grep -q '^<<<<<<<' CHANGELOG.md
changelog-merge check CHANGELOG.md

# Concurrent edits to the same entry must remain visible conflicts.
git checkout -b conflict-other "$base"
sed 's/Initial feature/Changed on other/' CHANGELOG.md > changelog.tmp
mv changelog.tmp CHANGELOG.md
git commit -am 'Edit on other'
git checkout -b conflict-main "$base"
sed 's/Initial feature/Changed on main/' CHANGELOG.md > changelog.tmp
mv changelog.tmp CHANGELOG.md
git commit -am 'Edit on main'
if git merge --no-edit conflict-other; then
  echo 'Expected conflicting edits to fail the merge' >&2
  exit 1
fi
grep -q '^<<<<<<<' CHANGELOG.md
grep -q '^|||||||' CHANGELOG.md
grep -Fx -- '- Changed on main.' CHANGELOG.md
grep -Fx -- '- Changed on other.' CHANGELOG.md
git merge --abort
cmp config-before .git/config
