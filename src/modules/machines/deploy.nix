{ pkgs, healthCheck ? "true", rollbackTimeout ? 300 }:
let
  check = pkgs.writeShellScript "devenv-machine-health-check" ''
    set -euo pipefail
    ${healthCheck}
  '';
in
pkgs.runCommand "devenv-machine-deploy"
{
  nativeBuildInputs = [ pkgs.makeWrapper ];
}
  ''
    mkdir -p $out/bin
    cp ${./deploy.py} $out/bin/devenv-machine-deploy
    sed -i '1i#!${pkgs.python3}/bin/python3' $out/bin/devenv-machine-deploy
    chmod +x $out/bin/devenv-machine-deploy
    wrapProgram $out/bin/devenv-machine-deploy \
      --set DEVENV_MACHINE_HEALTH_CHECK ${check} \
      --set DEVENV_MACHINE_ROLLBACK_TIMEOUT ${toString rollbackTimeout} \
      --prefix PATH : ${pkgs.lib.makeBinPath [ pkgs.nix pkgs.systemd pkgs.coreutils ]}
  ''
