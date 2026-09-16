{ config, lib, options, ... }:
let
  ssh = config.services.openssh;
  firewall = config.networking.firewall;
  admins = lib.filterAttrs (name: user: name == "root" || builtins.elem "wheel" user.extraGroups) config.users.users;
  facts = {
    version = 1;
    hostname = config.networking.hostName;
    ssh = {
      enabled = ssh.enable;
      ports = ssh.ports;
      rootLogin = ssh.settings.PermitRootLogin or "unknown";
      # Hash the declared entries instead of exporting their text: authorized
      # key options can contain command arguments or environment values.
      adminKeys = lib.mapAttrs
        (_: user:
          map (key: "sha256:" + builtins.hashString "sha256" key) user.openssh.authorizedKeys.keys
        )
        admins;
      dynamicKeys = lib.any (user: user.openssh.authorizedKeys.keyFiles != [ ]) (builtins.attrValues admins)
        || lib.any (path: path != "/etc/ssh/authorized_keys.d/%u") ssh.authorizedKeysFiles
        || (ssh.settings.AuthorizedKeysCommand or "none") != "none"
        || (ssh.settings.TrustedUserCAKeys or null) != null;
      # OpenSSH's own module renders standard settings through extraConfig.
      # Other definitions may contain Match blocks or override our facts.
      customConfig = lib.any
        (definition:
          !(lib.hasSuffix "/services/networking/ssh/sshd.nix" definition.file)
            && definition.value != ""
        )
        options.services.openssh.extraConfig.definitionsWithLocations
      || (ssh.settings.AllowUsers or null) != null || (ssh.settings.DenyUsers or null) != null
      || (ssh.settings.AllowGroups or null) != null || (ssh.settings.DenyGroups or null) != null;
    };
    firewall = {
      enabled = firewall.enable;
      allowedTCPPorts = firewall.allowedTCPPorts;
      allowedTCPPortRanges = firewall.allowedTCPPortRanges;
      customRules = firewall.extraCommands != "" || firewall.extraStopCommands != ""
        || firewall.extraInputRules != "" || firewall.extraForwardRules != ""
        || firewall.interfaces != { } || config.networking.nftables.enable;
    };
    recoveryEnabled = config.systemd.services.devenv-machines-recover.enable or false;
  };
in
{
  options.devenvMachineFacts = lib.mkOption {
    type = lib.types.attrs;
    internal = true;
    readOnly = true;
    description = "Versioned non-secret configuration facts for deployment access checks.";
  };
  config = {
    devenvMachineFacts = facts;
    environment.etc."devenv/machine-facts.json".text = builtins.toJSON facts;
  };
}
