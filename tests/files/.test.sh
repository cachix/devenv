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
# YAML renderers may include directives and document separators. Assert the
# generated document semantically instead of snapshotting their formatting.
[ "$(yq --output-format=json --indent=0 '.' foo.yaml)" = '{"foo":"bar"}' ]
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
