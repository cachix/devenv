{ pkgs, config, lib, ... }:

with lib;

let
  inherit (lib.attrsets) attrValues;

  cfg = config.languages.php;

  phps = config.lib.getInput {
    name = "phps";
    url = "github:fossar/nix-phps";
    attribute = "languages.php.version";
    follows = [ "nixpkgs" ];
  };

  filterDefaultExtensions = ext: builtins.length (builtins.filter (inner: inner == ext.extensionName) cfg.disableExtensions) == 0;

  configurePackage = package:
    package.buildEnv {
      extensions = { all, enabled }: with all; (builtins.filter filterDefaultExtensions (enabled ++ attrValues (getAttrs cfg.extensions package.extensions)));
      extraConfig = cfg.ini;
    };

  version = builtins.replaceStrings [ "." ] [ "" ] cfg.version;

  runtimeDir = config.env.DEVENV_STATE + "/php-fpm";

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

  startScript = pool: poolOpts: ''
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
            Path to the Unix socket file on which to accept FastCGI requests.

            This option is read-only and managed by NixOS.
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
            Options appended to the PHP configuration file `php.ini` used for this PHP-FPM pool.
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
            <https://www.php.net/manual/en/install.fpm.configuration.php">
            the manual for details. Note that settings names must be
            enclosed in quotes (e.g. `"pm.max_children"` instead of
            `pm.max_children`).
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
            See the documentation on `php-fpm.conf` for
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
    enable = lib.mkEnableOption "tools for PHP development";

    version = lib.mkOption {
      type = lib.types.str;
      default = "";
      description = "The PHP version to use.";
    };

    package = lib.mkOption {
      type = lib.types.package;
      default = configurePackage pkgs.php;
      defaultText = literalExpression "pkgs.php";
      description = ''
        Allows you to [override the default used package](https://nixos.org/manual/nixpkgs/stable/#ssec-php-user-guide)
        to adjust the settings or add more extensions. You can find the
        extensions using `devenv search 'php extensions'`
      '';
      example = literalExpression ''
        pkgs.php.buildEnv {
          extensions = { all, enabled }: with all; enabled ++ [ xdebug ];
          extraConfig = '''
            memory_limit=1G
          ''';
        };
      '';
    };

    packages = lib.mkOption {
      type = lib.types.submodule ({
        options = {
          composer = lib.mkOption {
            type = lib.types.nullOr lib.types.package;
            default = cfg.package.packages.composer;
            defaultText = lib.literalExpression "pkgs.phpPackages.composer";
            description = "composer package";
          };
        };
      });
      defaultText = lib.literalExpression "pkgs";
      default = { };
      description = "Attribute set of packages including composer";
    };

    ini = lib.mkOption {
      type = lib.types.nullOr lib.types.lines;
      default = "";
      description = ''
        PHP.ini directives. Refer to the "List of php.ini directives" of PHP's
      '';
    };

    extensions = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      description = ''
        PHP extensions to enable.
      '';
    };

    disableExtensions = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      description = ''
        PHP extensions to disable.
      '';
    };

    fpm = {
      settings = mkOption {
        type = with types; attrsOf (oneOf [ str int bool ]);
        default = {
          error_log = config.env.DEVENV_STATE + "/php-fpm/php-fpm.log";
        };
        description = ''
          PHP-FPM global directives. 
          
          Refer to the "List of global php-fpm.conf directives" section of
          <https://www.php.net/manual/en/install.fpm.configuration.php>
          for details. 
          
          Note that settings names must be enclosed in
          quotes (e.g. `"pm.max_children"` instead of `pm.max_children`). 
          
          You need not specify the options `error_log` or `daemonize` here, since
          they are already set.
        '';
      };

      extraConfig = mkOption {
        type = with types; nullOr lines;
        default = null;
        description = ''
          Extra configuration that should be put in the global section of
          the PHP-FPM configuration file. Do not specify the options
          `error_log` or `daemonize` here, since they are generated by
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
          Options appended to the PHP configuration file `php.ini`.
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

  config =
    let
      phpsPackage = phps.packages.${pkgs.stdenv.system}."php${version}" or (throw "PHP version ${cfg.version} is not available");
      nixpkgsPackageExists = (builtins.tryEval (toString pkgs."php${version}")).success;
      customPhpPackage = if ((builtins.hasAttr "php${version}" pkgs) && nixpkgsPackageExists) then pkgs."php${version}" else phpsPackage;
    in
    lib.mkIf cfg.enable {
      languages.php.package = lib.mkIf (cfg.version != "") (lib.mkForce (configurePackage customPhpPackage));

      languages.php.extensions = lib.optionals config.services.rabbitmq.enable [ "amqp" ]
        ++ lib.optionals config.services.redis.enable [ "redis" ]
        ++ lib.optionals config.services.blackfire.enable [ "blackfire" ];

      languages.php.ini = ''
        ${lib.optionalString config.services.mysql.enable ''
        pdo_mysql.default_socket = ''${MYSQL_UNIX_PORT}
        mysqli.default_socket = ''${MYSQL_UNIX_PORT}
        ''}
        ${lib.optionalString config.services.blackfire.enable ''
        blackfire.agent_socket = "${config.services.blackfire.socket}";
        ''}
      '';

      packages = with pkgs; [
        cfg.package
      ] ++ lib.optional (cfg.packages.composer != null) cfg.packages.composer;

      env.PHPFPMDIR = runtimeDir;

      processes = mapAttrs'
        (pool: poolOpts:
          nameValuePair "phpfpm-${pool}" {
            exec = startScript pool poolOpts;
          }
        )
        cfg.fpm.pools;
    };
}
