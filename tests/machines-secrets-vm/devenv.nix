{
  pkgs,
  inputs,
  config,
  ...
}:
let
  projectRoot = config.devenv.root;
  bootstrapManifest =
    config.machines.bootstrap._nixosEval.config.environment.etc."devenv/bootstrap-secrets.json".text;
  executor = import (inputs.devenv.modules + /machines/deploy.nix) {
    inherit pkgs;
    rollbackTimeout = 90;
    healthCheck = "true";
  };
  failingExecutor = import (inputs.devenv.modules + /machines/deploy.nix) {
    inherit pkgs;
    rollbackTimeout = 30;
    healthCheck = "false";
  };
  test = pkgs.testers.runNixOSTest {
    name = "devenv-machines-secrets";
    nodes.machine =
      {
        lib,
        config,
        extendModules,
        ...
      }:
      let
        activation =
          name: module:
          pkgs.writeShellScriptBin name
            (extendModules { modules = [ module ]; }).config.system.activationScripts.devenvSecrets.text;
      in
      {
        imports = [
          (inputs.devenv.modules + /machines/recovery.nix)
          (inputs.devenv.modules + /machines/facts.nix)
          (inputs.devenv.modules + /machines/secrets.nix)
        ];
        environment.systemPackages = [
          executor
          pkgs.jq
          (pkgs.writeShellScriptBin "devenv-test-activate-secrets" config.system.activationScripts.devenvSecrets.text)
          (activation "devenv-test-missing-owner" {
            devenv.secrets.APP_TOKEN.owner = lib.mkForce "missing-user";
          })
          (activation "devenv-test-missing-group" {
            devenv.secrets.APP_TOKEN.group = lib.mkForce "missing-group";
          })
          (activation "devenv-test-invalid-mode" {
            devenv.secrets.APP_TOKEN.mode = lib.mkForce "9999";
          })
          (activation "devenv-test-missing-secret" {
            devenv.secrets.Z_MISSING = { };
          })
        ];
        services.openssh.enable = true;
        environment.etc."devenv/bootstrap-secrets.json".text = bootstrapManifest;
        services.openssh.settings.PermitRootLogin = "prohibit-password";
        users.users.root.openssh.authorizedKeys.keys = [
          (lib.removeSuffix "\n" (builtins.readFile (projectRoot + "/.ssh-test-key.pub")))
        ];
        users.users.app = {
          isSystemUser = true;
          group = "app";
        };
        users.groups.app = { };
        devenv.secrets.APP_TOKEN = {
          owner = "app";
          restartUnits = [ "app.service" ];
        };
        specialisation.extra.configuration.devenv.secrets.EXTRA_TOKEN = { };
        specialisation.empty.configuration.devenv.secrets = lib.mkForce { };
        systemd.services.app = {
          wantedBy = [ "multi-user.target" ];
          serviceConfig = {
            User = "app";
            ExecStart = "${pkgs.coreutils}/bin/sleep infinity";
          };
        };
        virtualisation.memorySize = 2048;
        virtualisation.useBootLoader = true;
      };
    testScript = builtins.readFile ./test.py;
  };
in
{
  machines.bootstrap = {
    target.host = "root@bootstrap.example.com";
    nixos = import (pkgs.path + "/nixos/lib/eval-config.nix") {
      system = pkgs.stdenv.hostPlatform.system;
      modules = [ { system.stateVersion = "25.05"; } ];
    };
    install.secrets = {
      "/var/lib/sops-nix/key.txt" = {
        secret = "AGE_KEY";
      };
      "/var/lib/other/key.txt" = {
        secret = "AGE_KEY";
      };
    };
  };
  outputs.secrets-driver = test.driver;
  outputs.secrets-executor = executor;
  outputs.secrets-failing-executor = failingExecutor;
}
