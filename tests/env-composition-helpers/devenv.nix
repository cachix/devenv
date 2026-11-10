{
  config,
  inputs,
  pkgs,
  ...
}:
let
  env = config.lib.env;
  lateHook = pkgs.writeTextDir "nix-support/setup-hook" ''
    export ENV_SETUP_OBSERVED="$ENV_SETUP_INPUT"
    envTestLateHook() { export CC=package-cc; }
    postHooks+=( envTestLateHook )
  '';
in
{
  packages = [
    pkgs.nix
    pkgs.python3
    pkgs.bash
    pkgs.zlib
    pkgs.direnv
    pkgs.zsh
    pkgs.fish
    pkgs.nushell
    lateHook
  ];

  dotenv.enable = true;
  env = {
    CC = env.set "zig cc";
    CONFIG_SHELL = env.set "/configured/sh";
    PKG_CONFIG_PATH = env.prepend ":" [ "/configured/pkgconfig" ];
    NIX_CFLAGS_COMPILE = env.append " " [ "-DCUSTOM" ];
    TZ = env.set "Etc/GMT+3";
    SSL_CERT_FILE = env.set "/configured/ca.pem";
    ENV_SETUP_INPUT = env.beforeSetup "initial";
    ENV_ROOT = env.set "/configured";
    ENV_CHILD = config.env.ENV_ROOT + "/child";
    ENV_EMPTY = env.set "";
    ENV_TEST_CA_BUNDLE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
    ENV_SEARCH_PATH = env.combine [
      (env.beforeSetup "setup")
      (env.prepend ":" [ "first" ])
      (env.append ":" [ "last" ])
    ];
    ENV_ENTER_WIN = env.set "helper";
    ENV_TASK_WIN = env.set "helper";
    ENV_DEFAULT_EMPTY = env.combine [
      (env.beforeSetup "")
      (env.default "fallback")
    ];
  };

  enterShell = ''
    test "$ENV_ENTER_WIN" = helper
    export ENV_ENTER_WIN=enterShell
  '';
  tasks."devenv:enterShell".exports = [ "ENV_ENTER_WIN" ];
  tasks."test:override-helper" = {
    exec = ''
      test "$ENV_TASK_WIN" = helper
      export ENV_TASK_WIN=task
    '';
    exports = [ "ENV_TASK_WIN" ];
    before = [ "devenv:enterShell" ];
  };

  # Capture and activate explicit SDK overrides without changing the compiler
  # environment used by the zlib probe or building a custom Xcode installation.
  profiles.sdk.module.env = {
    DEVELOPER_DIR = env.set "/Applications/DevenvTestXcode.app/Contents/Developer";
    SDKROOT = env.set "/configured/MacOSX.sdk";
  };

  enterTest = ''
    test "$CC" = 'zig cc'
    test "$CONFIG_SHELL" = /configured/sh
    test "$TZ" = Etc/GMT+3
    test "$SSL_CERT_FILE" = /configured/ca.pem
    test "$ENV_SETUP_OBSERVED" = initial
    test "$ENV_CHILD" = /configured/child
    test "$ENV_EMPTY" = ""
    test "$ENV_DEFAULT_EMPTY" = ""
    case "$PKG_CONFIG_PATH" in /configured/pkgconfig:*) ;; *) exit 1;; esac
    pkg-config --exists zlib
    python3 ${./.}/check.py \
      --nixpkgs ${pkgs.path} \
      --library ${inputs.devenv.modules}/lib/env.nix
  '';
}
