{ pkgs
, lib
, config
, ...
}:

let
  domainList = lib.concatStringsSep " " config.certificates;
  hash = builtins.hashString "sha256" domainList;
  # mkcert needs tools that do not live in the nix store to write to the system
  # trust stores: security(1) and sudo(8) on darwin, sudo(8) on NixOS.
  # Other Linux distributions also need their own CA update commands.
  # The system PATH is not always inherited, for example with `devenv --clean`,
  # inside containers, or when the caller's PATH is already stripped.
  # Add those directories last, so nix store tools keep precedence.
  systemPath = lib.concatStringsSep ":" (
    lib.optionals pkgs.stdenv.hostPlatform.isDarwin [ "/usr/bin" "/bin" "/usr/sbin" "/sbin" ]
    ++ lib.optionals pkgs.stdenv.hostPlatform.isLinux [ "/run/wrappers/bin" "/usr/bin" "/bin" "/usr/sbin" "/sbin" ]
  );
  mkcert = pkgs.writeShellApplication {
    name = "mkcert";
    runtimeInputs = [ pkgs.nssTools pkgs.coreutils ];
    text = ''
      ${lib.optionalString (systemPath != "") ''export PATH="$PATH:${systemPath}"''}
      exec ${lib.getExe pkgs.mkcert} "$@"
    '';
  };
  setupScript = ''
    mkdir -p "${config.env.DEVENV_STATE}/mkcert"

    if [[ ! -f "$DEVENV_STATE/mkcert/rootCA.pem" ]]; then
      # `mkcert -install` creates the local CA and adds it to the system trust stores.
      # Trusting the CA needs privileges that are not always available, for example in CI.
      # The generated certificates stay usable without it, because env.CAROOT and
      # env.NODE_EXTRA_CA_CERTS point at the local CA.
      if ! ${lib.getExe mkcert} -install; then
        echo "devenv: mkcert could not add the local CA to the system trust stores." >&2
        echo "devenv: Certificates are still generated. Run 'mkcert -install' yourself to trust them." >&2
      fi

      if [[ ! -f "$DEVENV_STATE/mkcert/rootCA.pem" ]]; then
        echo "devenv: mkcert failed to create the local CA in $DEVENV_STATE/mkcert." >&2
        exit 1
      fi
    fi

    if [[ ! -f "$DEVENV_STATE/mkcert/hash" || "$(cat "$DEVENV_STATE/mkcert/hash")" != "${hash}" ]]; then
      echo "${hash}" > "${config.env.DEVENV_STATE}/mkcert/hash"

      pushd ${config.env.DEVENV_STATE}/mkcert > /dev/null

      ${lib.getExe mkcert} \
        ${lib.optionalString (config.keyFile != null) "-key-file ${config.keyFile}"} \
        ${lib.optionalString (config.certFile != null) "-cert-file ${config.certFile}"} \
        ${domainList} 2> /dev/null

      popd > /dev/null
    fi
  '';
  isNative = config.process.manager.implementation == "native";
  processTaskNames = lib.mapAttrsToList (name: _: "devenv:processes:${name}") config.processes;
  proxyHttps = config.process.proxy.enable && lib.any
    (process: process.proxy.https.enable && process.ports != { })
    (lib.attrValues config.processes);
in
{
  options = {
    certificates = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      description = "List of domains to generate certificates for.";
      example = [
        "example.com"
        "*.example.com"
      ];
    };
    certFile = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "Custom certificate file name, uses mkcert default if unset";
      example = "mycert.pem";
    };
    keyFile = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "Custom key file name, uses mkcert default if unset";
      example = "mykey.pem";
    };
  };

  config = lib.mkIf (domainList != "" || proxyHttps) {
    packages = [ mkcert ];

    changelogs = [
      {
        date = "2026-08-24";
        title = "mkcert no longer fails when the local CA cannot be trusted";
        description = ''
          `mkcert -install` needs `sudo` and, on darwin, `security` to add the local CA to the system trust stores.
          Those tools are unavailable on machines without a system PATH or without a way to gain privileges, such as CI runners.
          devenv now prints a warning instead of failing when the local CA cannot be trusted.
          The certificates are generated in either case, and `env.CAROOT` and `env.NODE_EXTRA_CA_CERTS` point at the local CA.
          devenv also adds the system directories (`/usr/bin`, `/bin`, `/usr/sbin`, `/sbin` on darwin, `/run/wrappers/bin` on NixOS) to the PATH of the mkcert calls, so the tools are found when they exist.
        '';
      }
    ];

    tasks."devenv:mkcert:setup" = lib.mkIf (isNative && domainList != "") {
      exec = setupScript;
      before = processTaskNames;
      description = "Generate TLS certificates with mkcert";
    };

    process.manager.before = lib.mkIf (!isNative && domainList != "") setupScript;

    env.CAROOT = "${config.env.DEVENV_STATE}/mkcert";
    env.DEVENV_MKCERT = lib.getExe mkcert;
    env.NODE_EXTRA_CA_CERTS = "${config.env.DEVENV_STATE}/mkcert/rootCA.pem";
  };
}
