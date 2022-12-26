{ pkgs, lib, config, ... }:

let
  hostContent = lib.concatStringsSep "\n" config.hosts;
  hostHash = builtins.hashString "sha256" hostContent;
  file = pkgs.writeText "hosts" ''
    # ${hostHash}
    ${hostContent}
  '';
in
{
  options = {
    hostsProfileName = lib.mkOption {
      type = lib.types.str;
      default = "devenv-${builtins.hashString "sha256" config.env.DEVENV_ROOT}";
      description = "Profile name to use.";
    };

    hosts = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      description = "List of hosts entries.";
    };
  };

  config = lib.mkIf (builtins.length config.hosts != 0) {
    packages = [
      (pkgs.writeShellScriptBin "deactivate-hosts" ''
        rm -f "$DEVENV_STATE/hostctl"
        exec sudo ${pkgs.hostctl}/bin/hostctl remove ${config.hostsProfileName} 
      ''
      )
    ];

    enterShell = ''
      if [[ ! -f "$DEVENV_STATE/hostctl" || "$(cat "$DEVENV_STATE/hostctl")" != "${hostHash}" ]]; then
        sudo ${pkgs.hostctl}/bin/hostctl replace ${config.hostsProfileName} --from ${file}
        echo "Hosts file updated. Run 'deactivate-hosts' to revert changes."
        mkdir -p "$DEVENV_STATE"
        echo "${hostHash}" > "$DEVENV_STATE/hostctl"
      fi
    '';
  };
}
