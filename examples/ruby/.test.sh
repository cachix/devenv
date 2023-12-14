#!/usr/bin/env bash
set -ex
ruby --version | grep "$(cat .ruby-version)"
ruby -e "puts RUBY_VERSION" | grep "$(cat .ruby-version)"
ruby -e "require 'puma'"
bundle --version
bundle exec ruby -e "puts RUBY_VERSION" | grep "$(cat .ruby-version)"
