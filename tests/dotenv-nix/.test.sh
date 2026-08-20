#!/usr/bin/env bash
set -ex

test "$BASE" = "first"
test "$QUOTED" = "two words"
test "$DERIVED_IN_NIX" = "first-two words"
test "$LOCAL_DERIVED_IN_NIX" = "missing"

sed -i 's/BASE=first/BASE=second/' .env
devenv shell bash -- -c 'test "$DERIVED_IN_NIX" = "second-two words"'

echo 'LOCAL=created' > .env.local
devenv shell bash -- -c 'test "$LOCAL_DERIVED_IN_NIX" = "created-nix"'

DOTENV_NIX_HOST=one devenv shell bash -- -c 'test "$HOST_DERIVED_IN_NIX" = "one-nix"'
DOTENV_NIX_HOST=two devenv shell bash -- -c 'test "$HOST_DERIVED_IN_NIX" = "two-nix"'
