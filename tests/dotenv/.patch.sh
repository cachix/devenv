#!/usr/bin/env bash
set -e

# Create local config
echo '{ env.LOCAL = "1";}' > devenv.local.nix

# Create .env files that dotenv module will read
cat <<EOF > .env
FOO=1
BAR=2
BAZ=3
export CHAZ=4
EOF
echo 'BAZ=5' > .env.bar
