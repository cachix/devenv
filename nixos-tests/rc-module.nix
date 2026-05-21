{ config, lib, ... }:

let
  inherit (lib) types mkOption mapAttrsToList concatStringsSep escapeShellArg;
in
{
  options.devenvTest = {
    user = mkOption {
      type = types.str;
      default = "dev";
    };

    home = mkOption {
      type = types.str;
      default = "/home/dev";
    };

    rcLines = mkOption {
      type = types.attrsOf (types.listOf types.str);
      default = { };
      description = ''
        Per-rc-file content lines keyed by basename (e.g. ".zshrc").
        Multiple imports merge via mkMerge.
      '';
    };
  };

  config = {
    system.activationScripts.devenvTestRc = ''
            install -d -o ${config.devenvTest.user} -g users -m 0755 ${config.devenvTest.home}
            ${concatStringsSep "\n" (mapAttrsToList (file: lines: ''
              cat > ${config.devenvTest.home}/${file} <<'DEVENV_TEST_RC_EOF'
      ${concatStringsSep "\n" lines}
      DEVENV_TEST_RC_EOF
              chown ${config.devenvTest.user}:users ${config.devenvTest.home}/${file}
              chmod 0644 ${config.devenvTest.home}/${file}
            '') config.devenvTest.rcLines)}
    '';
  };
}
