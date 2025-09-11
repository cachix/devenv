{ pkgs, ... }: {
  # Base configuration
  packages = [ pkgs.git pkgs.hello ];

  env.BASE_ENV = "base-value";

  # Profile definitions
  profiles."basic".config = {
    packages = [ pkgs.curl ];
    env.BASIC_PROFILE = "enabled";
  };

  profiles."backend".config = {
    packages = [ pkgs.wget pkgs.tree ];
    env.BACKEND_ENABLED = "true";
  };

  profiles."fast-startup".config = {
    packages = [ pkgs.hello ];
    env.FAST_STARTUP = "true";
  };

  profiles."extra-packages".config = {
    packages = [ pkgs.jq pkgs.htop ];
    env.EXTRA_TOOLS = "enabled";
  };

  # Profile merging test profiles
  profiles."profile-a".config = { lib, ... }: {
    packages = [ pkgs.curl pkgs.wget ];
    env.PROFILE_A = "active";
    env.MERGE_TEST = lib.mkDefault "profile-a";
  };

  profiles."profile-b".config = { lib, ... }: {
    packages = [ pkgs.jq pkgs.tree ];
    env.PROFILE_B = "active";
    env.MERGE_TEST = lib.mkForce "profile-b";
  };

  profiles."profile-c".config = { lib, ... }: {
    packages = [ pkgs.curl pkgs.jq pkgs.htop ];
    env.PROFILE_C = "active";
    env.MERGE_TEST = lib.mkForce "profile-c";
  };
}
