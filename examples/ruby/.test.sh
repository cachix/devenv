#!/bin/sh
set -ex
ruby --version | grep "$(cat .ruby-version)"
ruby -e "require 'puma'"
