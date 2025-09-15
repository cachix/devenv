{ pkgs, config, ... }:
{
  # Base configuration
  packages = [
    pkgs.git
    pkgs.hello
  ];

  env.BASE_ENV = "base-value";

  # Profile definitions
  profiles."basic".config = {
    packages = [ pkgs.curl ];
    env.BASIC_PROFILE = "enabled";
  };

  profiles."backend".config = {
    packages = [
      pkgs.wget
      pkgs.tree
    ];
    env.BACKEND_ENABLED = "true";
  };

  profiles."fast-startup".config = {
    packages = [ pkgs.hello ];
    env.FAST_STARTUP = "true";
  };

  profiles."extra-packages".config = {
    packages = [
      pkgs.jq
      pkgs.htop
    ];
    env.EXTRA_TOOLS = "enabled";
  };

  # Profile merging test profiles
  profiles."profile-a".config =
    { lib, ... }:
    {
      packages = [
        pkgs.curl
        pkgs.wget
      ];
      env.PROFILE_A = "active";
      env.MERGE_TEST = lib.mkDefault "profile-a";
    };

  profiles."profile-b".config =
    { pkgs, lib, ... }:
    {
      packages = [
        pkgs.jq
        pkgs.tree
      ];
      env.PROFILE_B = "active";
      env.MERGE_TEST = lib.mkForce "profile-b";
    };

  profiles."profile-c".config =
    { pkgs, lib, ... }:
    {
      packages = [
        pkgs.curl
        pkgs.jq
        pkgs.htop
      ];
      env.PROFILE_C = "active";
      env.MERGE_TEST = lib.mkForce "profile-c";
    };

  # Extends functionality tests
  profiles."base-profile".config =
    { lib, ... }:
    {
      packages = [
        pkgs.git
        pkgs.curl
      ];
      env.BASE_PROFILE = "enabled";
      env.EXTENDS_TEST = lib.mkDefault "base";
    };

  profiles."child-profile" = {
    extends = [ "base-profile" ];
    config =
      { lib, ... }:
      {
        packages = [ pkgs.wget ];
        env.CHILD_PROFILE = "enabled";
        env.EXTENDS_TEST = "child"; # Should override base (normal priority beats mkDefault)
      };
  };

  profiles."grandchild-profile" = {
    extends = [ "child-profile" ];
    config =
      { lib, ... }:
      {
        packages = [ pkgs.tree ];
        env.GRANDCHILD_PROFILE = "enabled";
        env.EXTENDS_TEST = lib.mkForce "grandchild"; # Should override child and base
      };
  };

  profiles."multiple-extends" = {
    extends = [
      "basic"
      "backend"
    ];
    config = {
      packages = [ pkgs.htop ];
      env.MULTIPLE_EXTENDS = "enabled";
    };
  };

  # Test hostname profile extends
  profiles.hostname."test-machine" = {
    extends = [ "base-profile" ];
    config = {
      env.HOSTNAME_PROFILE = "enabled";
    };
  };

  # Test user profile extends
  profiles.user."test-user" = {
    extends = [ "child-profile" ];
    config = {
      env.USER_PROFILE = "enabled";
    };
  };

  # Test priority conflicts - multiple profiles setting same env var
  profiles."conflict-low" = {
    config = {
      env.CONFLICT_VAR = "low-priority";
      env.CONFLICT_LOW = "enabled";
    };
  };

  profiles."conflict-high" = {
    config = {
      env.CONFLICT_VAR = "high-priority";
      env.CONFLICT_HIGH = "enabled";
    };
  };

  profiles."conflict-middle" = {
    config = {
      env.CONFLICT_VAR = "middle-priority";
      env.CONFLICT_MIDDLE = "enabled";
    };
  };

  # Test circular dependency - should cause infinite recursion
  profiles."cycle-a" = {
    extends = [ "cycle-b" ];
    config = {
      env.CYCLE_A = "enabled";
    };
  };

  profiles."cycle-b" = {
    extends = [ "cycle-a" ];
    config = {
      env.CYCLE_B = "enabled";
    };
  };

  # Test function vs attrset conflict
  profiles."function-profile" = {
    config =
      { ... }:
      {
        env.BASE_ENV = "foobar";
        env.TEST_VAR = "function";
      };
  };

  profiles."attrset-profile" = {
    extends = [ "function-profile" ];
    config =
      { config, ... }:
      {
        env.TEST_VAR = config.env.BASE_ENV;
      };
  };
}
