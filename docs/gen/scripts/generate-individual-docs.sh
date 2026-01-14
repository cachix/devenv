#!/usr/bin/env bash
set -ex

mkdir -p ../src/{languages,services,supported-process-managers}

# Build individual docs using the docs/gen devenv environment
result=$(devenv build outputs.devenv-generate-individual-docs | jq -r '."outputs.devenv-generate-individual-docs"')

cp -r --no-preserve=all "$result"/docs/individual-docs/* ../src/

# Move overview pages to index.md within their directories
mv ../src/languages.md ../src/languages/index.md
mv ../src/services.md ../src/services/index.md
