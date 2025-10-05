{ config, ... }:

{
  env.DEEP_LEVEL2 = "deep-level2";
  env.DEEP_LEVEL2_PATH = "tests/monorepo/deep/level1/level2";

  enterTest = ''
    echo "Testing DEEP LEVEL2 imports..."

    if [ "$COMMON" != "1" ]; then
      echo "ERROR: COMMON not loaded from /tests/monorepo/common"
      exit 1
    fi

    if [ "$SHARED" != "shared-module" ]; then
      echo "ERROR: SHARED not loaded from ../../../shared"
      exit 1
    fi

    if [ "$PROJECT_A" != "project-a" ]; then
      echo "ERROR: PROJECT_A not loaded from ../../../project-a"
      exit 1
    fi

    if [ "$DEEP_LEVEL1" != "deep-level1" ]; then
      echo "ERROR: DEEP_LEVEL1 not loaded from /tests/monorepo/deep/level1"
      exit 1
    fi

    echo "SUCCESS: All DEEP LEVEL2 imports loaded correctly"
  '';
}
