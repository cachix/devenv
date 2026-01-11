#!/usr/bin/env bash

set -xe

# Test evaluating a single boolean attribute
output=$(devenv eval languages.python.enable)
echo "$output" | jq -e '.["languages.python.enable"] == true'

# Test evaluating a single string attribute
output=$(devenv eval env.TEST_VAR)
echo "$output" | jq -e '.["env.TEST_VAR"] == "hello"'

# Test evaluating multiple attributes
output=$(devenv eval languages.python.enable env.TEST_VAR)
echo "$output" | jq -e '.["languages.python.enable"] == true'
echo "$output" | jq -e '.["env.TEST_VAR"] == "hello"'

# Test that enterShell contains expected content
output=$(devenv eval enterShell)
echo "$output" | jq -e '.["enterShell"] | contains("Welcome to the shell")'
