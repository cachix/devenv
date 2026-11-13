#!/usr/bin/env bash
set -euo pipefail
cat > .env <<'EOF'
CC=dotenv-cc
TZ=dotenv-tz
SSL_CERT_FILE=dotenv-ca
EOF
