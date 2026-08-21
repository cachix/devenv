{ pkgs, lib, ... }:

let
  testPackages = [
    pkgs.curl
    pkgs.jq
    pkgs.python3
  ];

  managerProfile = implementation: port: {
    process.manager.implementation = implementation;
    env.PROCESS_MANAGER_TEST_PORT = toString port;
  };

  # The lifecycle test must prove that mprocs is rejected from its declared
  # capabilities, before its executable is started. A marker-producing fake
  # makes that observable and avoids mprocs' platform-specific TTY/package
  # setup obscuring the capability check.
  fakeMprocs = pkgs.writeShellScriptBin "mprocs" ''
    touch "$PWD/mprocs-was-invoked"
    exit 42
  '';
in
{
  packages = testPackages;

  processes.test-server.exec = ''
    exec python3 -u -m http.server "$PROCESS_MANAGER_TEST_PORT" --bind 127.0.0.1
  '';

  profiles.native.module = managerProfile "native" 18770;
  profiles.process-compose.module = managerProfile "process-compose" 18771;
  profiles.honcho.module = managerProfile "honcho" 18772;
  profiles.hivemind.module = managerProfile "hivemind" 18773;
  profiles.overmind.module = managerProfile "overmind" 18774;

  profiles.mprocs.module = lib.recursiveUpdate (managerProfile "mprocs" 18775) {
    # Suppress mprocs' Darwin-only impure pbcopy shell dependency. The fake is
    # still referenced directly by process.manager.command, but `up -d` must
    # reject the launch before realizing or executing that command.
    packages = lib.mkForce testPackages;
    process.managers.mprocs.package = fakeMprocs;
  };
}
