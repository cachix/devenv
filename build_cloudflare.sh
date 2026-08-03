#!/usr/bin/env bash

set -xe

pushd docs

npm ci
npm run build

popd

mv docs/dist site
