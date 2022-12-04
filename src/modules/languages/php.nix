{ pkgs, config, lib, ... }:

with lib;

let
  cfg = config.languages.php;

  runtimeDir = "/tmp";

  toStr = value:
    if true == value then "yes"
    else if false == value then "no"
    else toString value;

  fpmCfgFile = pool: poolOpts: pkgs.writeText "phpfpm-${pool}.conf" ''
    [global]
    ${concatStringsSep "\n" (mapAttrsToList (n: v: "${n} = ${toStr v}") cfg.fpm.settings)}
    ${optionalString (cfg.fpm.extraConfig != null) cfg.fpm.extraConfig}
    [${pool}]
    ${concatStringsSep "\n" (mapAttrsToList (n: v: "${n} = ${toStr v}") poolOpts.settings)}
    ${concatStringsSep "\n" (mapAttrsToList (n: v: "env[${n}] = ${toStr v}") poolOpts.phpEnv)}
    ${optionalString (poolOpts.extraConfig != null) poolOpts.extraConfig}
  '';

  startScript = pool: poolOpts: pkgs.writeShellScriptBin "start-phpfpm" ''
    set -euo pipefail

    if [[ ! -d "$PHPFPMDIR" ]]; then
      mkdir -p "$PHPFPMDIR"
    fi

    exec ${poolOpts.phpPackage}/bin/php-fpm -F -y ${fpmCfgFile pool poolOpts} -c ${phpIni poolOpts}
  '';

  phpIni = poolOpts: pkgs.runCommand "php.ini"
    {
      inherit (poolOpts) phpPackage phpOptions;
      preferLocalBuild = true;
      passAsFile = [ "phpOptions" ];
    } ''
    cat ${poolOpts.phpPackage}/etc/php.ini $phpOptionsPath > $out
  '';

  poolOpts = { name, ... }:
    let
      poolOpts = cfg.fpm.pools.${name};
    in
    {
      options = {
        socket = mkOption {
          type = types.str;
          readOnly = true;
          description = ''
            Path to the unix socket file on which to accept FastCGI requests.
            <note><para>This option is read-only and managed by NixOS.</para></note>
          '';
          example = "${runtimeDir}/<name>.sock";
        };

        listen = mkOption {
          type = types.str;
          default = "";
          example = "/path/to/unix/socket";
          description = ''
            The address on which to accept FastCGI requests.
          '';
        };

        phpPackage = mkOption {
          type = types.package;
          default = cfg.package;
          defaultText = literalExpression "phpfpm.phpPackage";
          description = ''
            The PHP package to use for running this PHP-FPM pool.
          '';
        };

        phpOptions = mkOption {
          type = types.lines;
          description = ''
            "Options appended to the PHP configuration file <filename>php.ini</filename> used for this PHP-FPM pool."
          '';
        };

        phpEnv = lib.mkOption {
          type = with types; attrsOf str;
          default = { };
          description = ''
            Environment variables used for this PHP-FPM pool.
          '';
          example = literalExpression ''
            {
              HOSTNAME = "$HOSTNAME";
              TMP = "/tmp";
              TMPDIR = "/tmp";
              TEMP = "/tmp";
            }
          '';
        };

        settings = mkOption {
          type = with types; attrsOf (oneOf [ str int bool ]);
          default = { };
          description = ''
            PHP-FPM pool directives. Refer to the "List of pool directives" section of
            <link xlink:href="https://www.php.net/manual/en/install.fpm.configuration.php"/>
            for details. Note that settings names must be enclosed in quotes (e.g.
            <literal>"pm.max_children"</literal> instead of <literal>pm.max_children</literal>).
          '';
          example = literalExpression ''
            {
              "pm" = "dynamic";
              "pm.max_children" = 75;
              "pm.start_servers" = 10;
              "pm.min_spare_servers" = 5;
              "pm.max_spare_servers" = 20;
              "pm.max_requests" = 500;
            }
          '';
        };

        extraConfig = mkOption {
          type = with types; nullOr lines;
          default = null;
          description = ''
            Extra lines that go into the pool configuration.
            See the documentation on <literal>php-fpm.conf</literal> for
            details on configuration directives.
          '';
        };
      };

      config = {
        socket = if poolOpts.listen == "" then "${runtimeDir}/${name}.sock" else poolOpts.listen;
        phpOptions = mkBefore cfg.fpm.phpOptions;

        settings = mapAttrs (name: mkDefault) {
          listen = poolOpts.socket;
        };
      };
    };
in
{
  options.languages.php = {
    enable = lib.mkEnableOption "Enable tools for PHP development.";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.php;
      defaultText = "pkgs.php";
      description = lib.mdDoc ''
        Allows to [override the default used package](https://nixos.org/manual/nixpkgs/stable/#ssec-php-user-guide) to adjust the settings or add more extensions. You can find the extensions using `devenv search 'php extensions'`
        ```
      '';
      example = lib.literalExpression ''
        pkgs.php.buildEnv {
          extensions = { all, enabled }: with all; enabled ++ [ xdebug ];
          extraConfig = '''
            memory_limit=1G
          ''';
        };
      '';
    };

    fpm = {
      settings = mkOption {
        type = with types; attrsOf (oneOf [ str int bool ]);
        default = {
          error_log = config.env.DEVENV_STATE + "/php-fpm/php-fpm.log";
        };
        description = ''
          PHP-FPM global directives. Refer to the "List of global php-fpm.conf directives" section of
          <link xlink:href="https://www.php.net/manual/en/install.fpm.configuration.php"/>
          for details. Note that settings names must be enclosed in quotes (e.g.
          <literal>"pm.max_children"</literal> instead of <literal>pm.max_children</literal>).
          You need not specify the options <literal>error_log</literal> or
          <literal>daemonize</literal> here, since they are generated by NixOS.
        '';
      };

      extraConfig = mkOption {
        type = with types; nullOr lines;
        default = null;
        description = ''
          Extra configuration that should be put in the global section of
          the PHP-FPM configuration file. Do not specify the options
          <literal>error_log</literal> or
          <literal>daemonize</literal> here, since they are generated by
          NixOS.
        '';
      };

      phpOptions = mkOption {
        type = types.lines;
        default = "";
        example =
          ''
            date.timezone = "CET"
          '';
        description = ''
          Options appended to the PHP configuration file <filename>php.ini</filename>.
        '';
      };

      pools = mkOption {
        type = types.attrsOf (types.submodule poolOpts);
        default = { };
        example = literalExpression ''
          {
            mypool = {
              user = "php";
              group = "php";
              phpPackage = pkgs.php;
              settings = {
                "pm" = "dynamic";
                "pm.max_children" = 75;
                "pm.start_servers" = 10;
                "pm.min_spare_servers" = 5;
                "pm.max_spare_servers" = 20;
                "pm.max_requests" = 500;
              };
            }
          }'';
        description = ''
          PHP-FPM pools. If no pools are defined, the PHP-FPM
          service is disabled.
        '';
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      cfg.package
      cfg.package.packages.composer
    ];

    env.PHPFPMDIR = config.env.DEVENV_STATE + "/php-fpm";

    enterShell = ''
      php --version
      composer --version
    '';

    processes = mapAttrs'
      (pool: poolOpts:
        nameValuePair "phpfpm-${pool}" {
          exec = "${startScript pool poolOpts}/bin/start-phpfpm";
        }
      )
      cfg.fpm.pools;
  };
}
