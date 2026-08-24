{ pkgs, lib, config, ... }:

let
  reducerFn = (prev: curr: prev ++ (if builtins.typeOf curr.ip == "string" then [ curr ] else builtins.map (ip: { inherit ip; hostname = curr.hostname; }) curr.ip));
  reducer = lib.lists.foldl reducerFn [ ];
  entries = lib.mapAttrsToList (hostname: ip: { inherit hostname ip; }) config.hosts;
  separateEntriesWithIps = reducer entries;
  entriesByIp = builtins.groupBy ({ ip, ... }: ip) separateEntriesWithIps;
  hostnamesByIp = builtins.mapAttrs (hostname: entries: builtins.map ({ hostname, ... }: hostname) entries) entriesByIp;
  lines = lib.mapAttrsToList (ip: hostnames: "${ip} ${lib.concatStringsSep " " hostnames}") hostnamesByIp;
  hostContent = lib.concatStringsSep "\n" lines;
  hostHash = builtins.hashString "sha256" hostContent;
  file = pkgs.writeText "hosts" ''
    # ${hostHash}
    ${hostContent}
  '';
  # sudo does not live in the nix store: it is in /usr/bin on darwin and in
  # /run/wrappers/bin on NixOS. The system PATH is not always inherited, for
  # example with `devenv --clean`, inside containers, or when the caller's PATH
  # is already stripped. Add those directories last, so nix store tools keep
  # precedence.
  systemPath = lib.concatStringsSep ":" (
    lib.optionals pkgs.stdenv.hostPlatform.isDarwin [ "/usr/bin" "/bin" "/usr/sbin" "/sbin" ]
    ++ lib.optionals pkgs.stdenv.hostPlatform.isLinux [ "/run/wrappers/bin" "/usr/bin" "/bin" ]
  );
  # Run hostctl with the privileges needed to write /etc/hosts.
  # Returns non-zero with an explanation when that is not possible.
  elevate = ''
    devenv_hostctl() {
      local PATH="$PATH${lib.optionalString (systemPath != "") ":${systemPath}"}"

      if [[ $EUID -eq 0 ]]; then
        ${pkgs.hostctl}/bin/hostctl "$@"
      elif command -v sudo > /dev/null; then
        sudo ${pkgs.hostctl}/bin/hostctl "$@"
      else
        echo "devenv: cannot update /etc/hosts, because 'sudo' is not available." >&2
        echo "devenv: Add these entries to /etc/hosts yourself:" >&2
        ${lib.concatMapStringsSep "\n    " (line: "echo ${lib.escapeShellArg line} >&2") lines}
        return 1
      fi
    }
  '';
  setupScript = ''
    ${elevate}
    if [[ ! -f "$DEVENV_STATE/hostctl" || "$(cat "$DEVENV_STATE/hostctl")" != "${hostHash}" ]]; then
      devenv_hostctl replace ${config.hostsProfileName} --from ${file}
      mkdir -p "$DEVENV_STATE"
      echo "${hostHash}" > "$DEVENV_STATE/hostctl"
    fi
  '';
  teardownScript = ''
    ${elevate}
    rm -f "$DEVENV_STATE/hostctl"
    devenv_hostctl remove ${config.hostsProfileName}
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
        "another-example.com" = [ "::1" "127.0.0.1" ];
      };
    };
  };

  config = lib.mkIf (hostContent != "") {
    tasks."devenv:hostctl:setup" = lib.mkIf isNative {
      exec = setupScript;
      # Soft dependency: process tasks still start if hostctl fails
      # (e.g. /etc/hosts is read-only on NixOS).
      before = map (name: "${name}@completed") processTaskNames;
      description = "Configure /etc/hosts entries with hostctl";
    };

    process.manager.before = lib.mkIf (!isNative) setupScript;
    process.manager.after = lib.mkIf (!isNative) teardownScript;
  };
}
