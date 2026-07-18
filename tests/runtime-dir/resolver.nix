{ repo }:
let
  runtimeDir = import (builtins.toPath "${repo}/src/modules/runtime-dir.nix");
  dotfile = "/home/test/project/.devenv";
  uid = "1234";
  hash = builtins.substring 0 7 (builtins.hashString "sha256" dotfile);
  env =
    values:
    name: values.${name} or "";
  resolve =
    values: existing:
    runtimeDir.resolve {
      inherit dotfile uid;
      getEnv = env values;
      pathExists = path: builtins.elem path existing;
    };
in
assert resolve { } [ "/run/user/1234" ] == "/run/user/1234/devenv-${hash}";
assert resolve { } [ ] == "/tmp/devenv-1234-${hash}";
assert
resolve
  { TMPDIR = "/legacy/tmp"; }
  [
    "/run/user/1234"
    "/legacy/tmp/devenv-${hash}/processes/native.sock"
  ]
  == "/legacy/tmp/devenv-${hash}";
assert
resolve
  { TMPDIR = "/unrelated/tmp"; }
  [ "/run/user/1234" ]
  == "/run/user/1234/devenv-${hash}";
assert
resolve
  {
    XDG_RUNTIME_DIR = "/xdg";
    TMPDIR = "/legacy/tmp";
  }
  [ "/legacy/tmp/devenv-${hash}/processes/native.sock" ]
  == "/xdg/devenv-${hash}";
true
