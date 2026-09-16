{ pkgs, ... }:
{
  # The rooted executor belongs to the recorded transaction, even if the
  # newly booted configuration was built by a different devenv version.
  systemd.services.devenv-machines-recover = {
    description = "Recover an unconfirmed devenv machine deployment after reboot";
    wantedBy = [ "multi-user.target" ];
    after = [ "local-fs.target" ];
    restartIfChanged = false;
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
      UMask = "0077";
      ExecStart = pkgs.writeShellScript "devenv-machines-recover" ''
        executor=/nix/var/nix/gcroots/devenv-machines/executor/bin/devenv-machine-deploy
        if test -e /var/lib/devenv-machines/current.json; then
          exec "$executor" recover
        fi
      '';
    };
  };
}
