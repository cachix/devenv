{ config, lib, pkgs, ... }:

let
  cfg = config.services.trafficserver;

  getManualUrl = name:
    "https://docs.trafficserver.apache.org/en/latest/admin-guide/files/${name}.en.html";

  yaml = pkgs.formats.yaml { };

  writeYAML = name: cfg:
    if cfg == null
    then pkgs.emptyFile.overrideAttrs (_: { inherit name; })
    else yaml.generate name cfg;

  writeLines = name: lines:
    let
      allLines = lines ++ lib.optional (lines != [ ]) "";
      text = lib.concatStringsSep "\n" allLines;
    in
    pkgs.writeText name text;

  mkRecordLine = setting: type: value:
    let
      key =
        if lib.last setting == "_"
        then lib.concatStringsSep "." (lib.init setting)
        else lib.concatStringsSep "." setting;
    in
    "CONFIG ${key} ${type} ${toString value}";

  toRecords = setting: value:
    if lib.isAttrs value then
      let
        toLines = lines: n: v:
          assert !lib.hasInfix "." n;
          lines ++ (toRecords (setting ++ [ n ]) v);
      in
      lib.foldlAttrs toLines [ ] value
    else if lib.isInt value then
      [ (mkRecordLine setting "INT" value) ]
    else if lib.isFloat value then
      [ (mkRecordLine setting "FLOAT" value) ]
    else if value == null then
      [ (mkRecordLine setting "STRING" "NULL") ]
    else
      [ (mkRecordLine setting "STRING" value) ];

  writeRecords = name: cfg:
    writeLines name (toRecords [ ] cfg);

  writePluginConfig = name: cfg:
    writeLines name (map (p: "${p.path} ${p.arg}") cfg);

  confdir = pkgs.linkFarmFromDrvs "trafficserver-config" [
    (pkgs.writeText "cache.config" cfg.cache)
    (pkgs.writeText "hosting.config" cfg.hosting)
    (pkgs.writeText "parent.config" cfg.parent)
    (pkgs.writeText "remap.config" cfg.remap)
    (pkgs.writeText "splitdns.config" cfg.splitDns)
    (pkgs.writeText "ssl_multicert.config" cfg.sslMulticert)
    (pkgs.writeText "storage.config" cfg.storage)
    (pkgs.writeText "volume.config" cfg.volume)
    (writeYAML "logging.yaml" cfg.logging)
    (writeYAML "sni.yaml" cfg.sni)
    (writeYAML "strategies.yaml" cfg.strategies)
    (writeYAML "ip_allow.yaml" cfg.ipAllow)
    (writeRecords "records.config" cfg.records)
    (writePluginConfig "plugin.config" cfg.plugins)
  ];

  statedir = "${config.env.DEVENV_STATE}/trafficserver";
  runroot = {
    prefix = statedir;
    exec_prefix = statedir;
    sysconfdir = confdir;
    datadir = "${statedir}/share";
    localstatedir = "${statedir}/state";
    runtimedir = "${config.env.DEVENV_RUNTIME}/trafficserver";
    logdir = "${statedir}/log";
    cachedir = "${statedir}/cache";
    bindir = "${cfg.package}/bin";
    sbindir = "${cfg.package}/bin";
    includedir = "${cfg.package}/include";
    libdir = "${cfg.package}/lib";
    libexecdir = "${cfg.package}/libexec";
  };

  inherit (lib) types;
in
{
  options.services.trafficserver = {
    enable = lib.mkEnableOption "Apache Traffic Server";

    package = lib.mkOption {
      type = types.package;
      default = pkgs.trafficserver;
      description = "Apache Traffic Server package";
    };

    runroot = lib.mkOption {
      readOnly = true;
      default = runroot;
      description = "File layout used by Traffic Server";
    };

    cache = lib.mkOption {
      type = types.lines;
      default = "";
      example = "dest_domain=example.com suffix=js action=never-cache";
      description = ''
        Caching rules that overrule the origin's caching policy.

        Consult the [upstream documentation](${getManualUrl "cache.config"})
        for more details.
      '';
    };

    hosting = lib.mkOption {
      type = types.lines;
      default = "";
      example = "domain=example.com volume=1";
      description = ''
        Partition the cache according to origin server or domain

        Consult the [upstream documentation](${getManualUrl "hosting.config"})
        for more details.
      '';
    };

    ipAllow = lib.mkOption {
      type = types.nullOr yaml.type;
      default = lib.importJSON ./ip_allow.json;
      defaultText = lib.literalMD "upstream defaults";
      example = lib.literalExpression ''
        {
          ip_allow = [{
            apply = "in";
            ip_addrs = "127.0.0.1";
            action = "allow";
            methods = "ALL";
          }];
        }
      '';
      description = ''
        Control client access to Traffic Server and Traffic Server connections
        to upstream servers.

        Consult the [upstream documentation](${getManualUrl "ip_allow.yaml"})
        for more details.
      '';
    };

    logging = lib.mkOption {
      type = types.nullOr yaml.type;
      default = lib.importJSON ./logging.json;
      defaultText = lib.literalMD "upstream defaults";
      example = { };
      description = ''
        Configure logs.

        Consult the [upstream documentation](${getManualUrl "logging.yaml"})
        for more details.
      '';
    };

    parent = lib.mkOption {
      type = types.lines;
      default = "";
      example = ''
        dest_domain=. method=get parent="p1.example:8080; p2.example:8080" round_robin=true
      '';
      description = ''
        Identify the parent proxies used in an cache hierarchy.

        Consult the [upstream documentation](${getManualUrl "parent.config"})
        for more details.
      '';
    };

    plugins = lib.mkOption {
      default = [ ];

      description = ''
        Controls run-time loadable plugins available to Traffic Server, as
        well as their configuration.

        Consult the [upstream documentation](${getManualUrl "plugin.config"})
        for more details.
      '';

      type = with types;
        listOf (submodule {
          options.path = lib.mkOption {
            type = str;
            example = "xdebug.so";
            description = ''
              Path to plugin. The path can either be absolute, or relative to
              the plugin directory.
            '';
          };
          options.arg = lib.mkOption {
            type = str;
            default = "";
            example = "--header=ATS-My-Debug";
            description = "arguments to pass to the plugin";
          };
        });
    };

    records = lib.mkOption {
      type = with types;
        let
          valueType = (attrsOf (oneOf [ int float str valueType ])) // {
            description = "Traffic Server records value";
          };
        in
        valueType;
      default = { };
      example = { proxy.config.proxy_name = "my_server"; };
      description = ''
        List of configurable variables used by Traffic Server.

        Consult the [upstream documentation](${getManualUrl "records.config"})
        for more details.

        When defining the values for the option `x.y`, a nested attribute should
        be used. Using a flat attribute set with the attribute name `x.y` will
        result in an error.

        If options for both `x.y` and `x.y.z` needs to be set, you can set
        `x.y._` as `x.y`. This only applies to Traffic Server versions prior to
        10. Traffic Server 10 and onwards uses YAML configuration, which doesn't
        have this kind of problem.
      '';
    };

    remap = lib.mkOption {
      type = types.lines;
      default = "";
      example = "map http://from.example http://origin.example";
      description = ''
        URL remapping rules used by Traffic Server.

        Consult the [upstream documentation](${getManualUrl "remap.config"})
        for more details.
      '';
    };

    splitDns = lib.mkOption {
      type = types.lines;
      default = "";
      example = ''
        dest_domain=internal.corp.example named="255.255.255.255:212 255.255.255.254" def_domain=corp.example search_list="corp.example corp1.example"
        dest_domain=!internal.corp.example named=255.255.255.253
      '';
      description = ''
        Specify the DNS server that Traffic Server should use under specific
        conditions.

        Consult the [upstream documentation](${getManualUrl "splitdns.config"})
        for more details.
      '';
    };

    sslMulticert = lib.mkOption {
      type = types.lines;
      default = "";
      example = "dest_ip=* ssl_cert_name=default.pem";
      description = ''
        Configure SSL server certificates to terminate the SSL sessions.

        Consult the [upstream documentation](${getManualUrl "ssl_multicert.config"})
        for more details.
      '';
    };

    sni = lib.mkOption {
      type = types.nullOr yaml.type;
      default = null;
      example = lib.literalExpression ''
        {
          sni = [{
            fqdn = "no-http2.example.com";
            https = "off";
          }];
        }
      '';
      description = ''
        Configure aspects of TLS connection handling for both inbound and
        outbound connections.

        Consult the [upstream documentation](${getManualUrl "sni.yaml"})
        for more details.
      '';
    };

    storage = lib.mkOption {
      type = types.lines;
      default = "/var/cache/trafficserver 256M";
      example = "/dev/disk/by-id/XXXXX volume=1";
      description = ''
        List all the storage that make up the Traffic Server cache.

        Consult the [upstream documentation](${getManualUrl "storage.config"})
        for more details.
      '';
    };

    strategies = lib.mkOption {
      type = types.nullOr yaml.type;
      default = null;
      description = ''
        Specify the next hop proxies used in an cache hierarchy and the
        algorithms used to select the next proxy.

        Consult the [upstream documentation](${getManualUrl "strategies.yaml"})
        for more details.
      '';
    };

    volume = lib.mkOption {
      type = types.nullOr yaml.type;
      default = "";
      example = "volume=1 scheme=http size=20%";
      description = ''
        Manage cache space more efficiently and restrict disk usage by
        creating cache volumes of different sizes.

        Consult the [upstream documentation](${getManualUrl "volume.config"})
        for more details.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [ cfg.package ];

    env.TS_RUNROOT = writeYAML "runroot.yaml" runroot;

    processes.trafficserver.exec =
      let
        q = lib.escapeShellArg;
        qs = lib.escapeShellArgs;
      in
      ''
        set -euxo pipefail

        mkdir -p ${qs (with runroot; [
          datadir localstatedir runtimedir logdir cachedir ])}
        cd ${q runroot.prefix}

        rm ${q "${statedir}/config"} ||:
        ln -s ${q confdir} ${q "${statedir}/config"}

        exec ${cfg.package}/bin/traffic_manager
      '';

    services.trafficserver.records.proxy.config.body_factory.template_sets_dir =
      lib.mkDefault "${cfg.package}/etc/trafficserver/body_factory";
  };
}
