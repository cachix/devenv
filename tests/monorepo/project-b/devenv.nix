{ config, ... }:

{
  env.PROJECT_B = "project-b";
  env.PROJECT_B_PATH = "tests/monorepo/project-b";
  env.PROJECT_B_FROM_A = config.env.PROJECT_A or "unset";
  env.PROJECT_B_FROM_SUBMODULE = config.env.SUBMODULE_A or "unset";

  enterTest = ''
    echo "Testing PROJECT-B imports..."
    if [ "$SHARED" != "shared-module" ]; then
      echo "ERROR: SHARED not loaded from ../shared"
      exit 1
    fi

    if [ "$COMMON" != "1" ]; then
      echo "ERROR: COMMON not loaded from /tests/monorepo/common"
      exit 1
    fi

    if [ "$PROJECT_A" != "project-a" ]; then
      echo "ERROR: PROJECT_A not loaded from ../project-a"
      exit 1
    fi

    if [ "$SUBMODULE_A" != "submodule-a" ]; then
      echo "ERROR: SUBMODULE_A not loaded from ../project-a/submodule"
      exit 1
    fi

    echo "SUCCESS: All PROJECT-B imports loaded correctly"
  '';
}
