{
  pkgs,
  lib,
  config,
  ...
}:

let
  reducerFn = (
    prev: curr:
    prev
    ++ (
      if builtins.typeOf curr.ip == "string" then
        [ curr ]
      else
        builtins.map (ip: {
          inherit ip;
          hostname = curr.hostname;
        }) curr.ip
    )
  );
  reducer = lib.lists.foldl reducerFn [ ];
  entries = lib.mapAttrsToList (hostname: ip: { inherit hostname ip; }) config.hosts;
  separateEntriesWithIps = reducer entries;
  entriesByIp = builtins.groupBy ({ ip, ... }: ip) separateEntriesWithIps;
  hostnamesByIp = builtins.mapAttrs (
    hostname: entries: builtins.map ({ hostname, ... }: hostname) entries
  ) entriesByIp;
  lines = lib.mapAttrsToList (
    ip: hostnames: "${ip} ${lib.concatStringsSep " " hostnames}"
  ) hostnamesByIp;
  hostContent = lib.concatStringsSep "\n" lines;
  hostHash = builtins.hashString "sha256" hostContent;
  file = pkgs.writeText "hosts" ''
    # ${hostHash}
    ${hostContent}
  '';
  setupScript = ''
    if [[ ! -f "$DEVENV_STATE/hostctl" || "$(cat "$DEVENV_STATE/hostctl")" != "${hostHash}" ]]; then
      sudo ${pkgs.hostctl}/bin/hostctl replace ${config.hostsProfileName} --from ${file}
      mkdir -p "$DEVENV_STATE"
      echo "${hostHash}" > "$DEVENV_STATE/hostctl"
    fi
  '';
  teardownScript = ''
    rm -f "$DEVENV_STATE/hostctl"
    sudo ${pkgs.hostctl}/bin/hostctl remove ${config.hostsProfileName}
  '';
  isNative = config.process.manager.implementation == "native";
  processTaskNames = lib.mapAttrsToList (name: _: "devenv:processes:${name}") config.processes;
in
{
  options = {
    hostsProfileName = lib.mkOption {
      type = lib.types.str;
      default = "devenv-${builtins.hashString "sha256" config.env.DEVENV_ROOT}";
      defaultText = "devenv-<hash>";
      description = "Profile name to use.";
    };

    hosts = lib.mkOption {
      type = lib.types.attrsOf (lib.types.either lib.types.str (lib.types.listOf lib.types.str));
      default = { };
      description = "List of hosts entries.";
      example = {
        "example.com" = "127.0.0.1";
        "another-example.com" = [
          "::1"
          "127.0.0.1"
        ];
      };
    };
  };

  config = lib.mkIf (hostContent != "") {
    tasks."devenv:hostctl:setup" = lib.mkIf isNative {
      exec = setupScript;
      before = processTaskNames;
      description = "Configure /etc/hosts entries with hostctl";
    };

    process.manager.before = lib.mkIf (!isNative) setupScript;
    process.manager.after = lib.mkIf (!isNative) teardownScript;
  };
}
