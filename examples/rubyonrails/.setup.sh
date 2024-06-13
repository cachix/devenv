#!/usr/bin/env bash
set -ex

gem install rails

if [ ! -d "blog" ]; then
  rails new blog --database=postgresql --force
fi

pushd blog
  bundle
popd
