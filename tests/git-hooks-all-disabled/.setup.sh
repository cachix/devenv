if ! test -f "$DEVENV_ROOT/.pre-commit-config.yaml"; then
  echo "Test not setup correctly: .pre-commit-config.yaml not found"
  exit 1
fi

echo "{ lib, ... }: { git-hooks.hooks.no-op.enable = lib.mkForce false; }" > devenv.local.nix
