{ config, ... }:

{
  env.MIXED = "mixed-imports";
  env.MIXED_PATH = "tests/monorepo/mixed-imports";

  enterTest = ''
    echo "Testing MIXED imports with .nix extensions..."

    if [ "$SHARED" != "shared-module" ]; then
      echo "ERROR: SHARED not loaded from ../shared/devenv.nix"
      exit 1
    fi

    if [ "$COMMON" != "1" ]; then
      echo "ERROR: COMMON not loaded from /tests/monorepo/common/devenv.nix"
      exit 1
    fi

    if [ "$EXTRA_MIXED" != "extra-mixed" ]; then
      echo "ERROR: EXTRA_MIXED not loaded from ./extra.nix"
      exit 1
    fi

    if [ "$PROJECT_A" != "project-a" ]; then
      echo "ERROR: PROJECT_A not loaded from ../project-a"
      exit 1
    fi

    echo "SUCCESS: All MIXED imports with .nix extensions loaded correctly"
  '';
}
