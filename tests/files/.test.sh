#!/usr/bin/env bash

set -xe

function assert_file() {
  test -f "$1"
  [ "$(cat)" = "$(cat "$1")" ]
}

assert_file foo.txt <<EOF
foo
EOF
assert_file foo.ini <<EOF
[foo]
bar=baz
EOF
assert_file foo.yaml <<EOF
foo: bar
EOF
assert_file foo.toml <<EOF
foo = "bar"
EOF
assert_file foo.json <<EOF
{
  "foo": "bar"
}
EOF

assert_file dir/foo.txt <<EOF
foo
EOF

# Test executable flag
test -x script.sh
assert_file script.sh <<EOF
#!/bin/bash
echo hello
EOF

# Verify state tracking
test -f .devenv/state/files.json
