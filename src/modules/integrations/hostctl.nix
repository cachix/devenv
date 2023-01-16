{ pkgs, lib, config, ... }:

let
  entries = lib.mapAttrsToList (hostname: ip: { inherit hostname ip; }) config.hosts;
  entriesByIp = builtins.groupBy ({ ip, ... }: ip) entries;
  hostnamesByIp = builtins.mapAttrs (hostname: entries: builtins.map ({ hostname, ... }: hostname) entries) entriesByIp;
  lines = lib.mapAttrsToList (ip: hostnames: "${ip} ${lib.concatStringsSep " " hostnames}") hostnamesByIp;
  hostContent = lib.concatStringsSep "\n" lines;
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
      type = lib.types.attrsOf lib.types.str;
      default = [ ];
      description = "List of hosts entries.";
      example = {
        "example.com" = "127.0.0.1";
      };
    };
  };

  config = lib.mkIf (hostContent != "") {
    beforeUp = ''
      if [[ ! -f "$DEVENV_STATE/hostctl" || "$(cat "$DEVENV_STATE/hostctl")" != "${hostHash}" ]]; then
        sudo ${pkgs.hostctl}/bin/hostctl replace ${config.hostsProfileName} --from ${file}
        echo "Hosts file updated. Run 'deactivate-hosts' to revert changes."
        mkdir -p "$DEVENV_STATE"
        echo "${hostHash}" > "$DEVENV_STATE/hostctl"
      fi
    '';

    afterUp = ''
      rm -f "$DEVENV_STATE/hostctl"
      sudo ${pkgs.hostctl}/bin/hostctl remove ${config.hostsProfileName} 
    '';
  };
}
