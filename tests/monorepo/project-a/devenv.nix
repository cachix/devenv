{ config, ... }:

{
  env.PROJECT_A = "project-a";
  env.PROJECT_A_PATH = "tests/monorepo/project-a";

  # Verify all imports loaded correctly
  enterTest = ''
    echo "Testing PROJECT-A imports..."

    # Check shared module (upward path)
    if [ "$SHARED" != "shared-module" ]; then
      echo "ERROR: SHARED not loaded from ../shared"
      exit 1
    fi

    # Check common module (absolute path)
    if [ "$COMMON" != "1" ]; then
      echo "ERROR: COMMON not loaded from /tests/monorepo/common"
      exit 1
    fi

    # Check submodule (local path) - only when defined
    if [ -n "''${SUBMODULE_A:-}" ]; then
      if [ "$SUBMODULE_A" != "submodule-a" ]; then
        echo "ERROR: SUBMODULE_A not loaded from ./submodule"
        exit 1
      fi
    fi

    echo "SUCCESS: All PROJECT-A imports loaded correctly"
  '';
}
