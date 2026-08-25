set -e

wait_for_port 3000

spacetime login --server-issued-login local
spacetime login show
spacetime server ping local
