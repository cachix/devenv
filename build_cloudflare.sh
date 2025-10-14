#!/usr/bin/env bash

set -xe

pushd docs

pip install -r requirements.txt
mkdocs build
cp _redirects site/

popd

mv docs/site site
