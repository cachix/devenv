echo '{ env.LOCAL = "1";}' > devenv.local.nix
cat <<EOF > .env
FOO=1
BAR=2
BAZ=3
export CHAZ=4
QUOTED="two words"
JSON={"enabled":true}
INLINE_COMMENT=value # ignored comment
MULTILINE="first
second"
A=0
B=1
EXPANDED=\$A+\$B
FROM_HOST=\$USER
LITERAL_DOLLAR='\$2a\$10\$hash'
DISABLED=dotenv
MUTATED_BY_TASK=before
REMOVED_BY_TASK=present
NIX_OWNED_SAME=before
SHELL=/dotenv/shell
DEVENV_CMDLINE=dotenv
PATH=/dotenv/bin
XDG_DATA_DIRS=/dotenv/share
EOF
mkdir -p config
echo 'BAZ=5' > config/.env.bar
