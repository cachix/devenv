#!/usr/bin/env bash

set -xe

test -f .gitignore

cat > expected.gitignore <<'EOF'
###------------------------------###
###  File: ./template.gitignore  ###
###------------------------------###

# gitnr test template
template-only-line

###--------------------###
###  File: /dev/stdin  ###
###--------------------###

*.log
dist/
EOF

diff -u expected.gitignore .gitignore >&2
