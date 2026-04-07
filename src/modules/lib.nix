{ pkgs, lib, config, inputs, ... }:

{
  # freestyle
  options.lib = lib.mkOption {
    type = lib.types.attrsOf lib.types.anything;
    internal = true;
  };

  config.lib = {
    # Priority 500: between mkDefault (1000) and actual default (100)
    mkOverrideDefault = lib.mkOverride 500;
    # Internal function to format input requirement messages.
    # Underscore prefix signals this is not part of the public API.
    _mkInputError = { name, url, attribute, follows ? [ ] }:
      let
        flags = lib.concatStringsSep " " (map (i: "--follows ${i}") follows);
        yaml_follows = lib.concatStringsSep "\n      " (map (i: "${i}:\n        follows: ${i}") follows);
        command =
          if config.devenv.flakesIntegration
          then ''
            Add the following to flake.nix:

            inputs.${name}.url = "${url}";
            ${if follows != []
              then "inputs.${name}.inputs = { ${lib.concatStringsSep "; " (map (i: "${i}.follows = \"${i}\"") follows)}; };"
              else ""}
          ''
          else if config.devenv.cli.version != null && lib.versionAtLeast config.devenv.cli.version "1.0"
          then ''
            run the following command:

              $ devenv inputs add ${name} ${url} ${flags}
          ''
          else ''
            add the following to your devenv.yaml:

              ✨ devenv 1.0 made this easier: https://devenv.sh/getting-started/#installation ✨

              inputs:
                ${name}:
                  url: ${url}
                  ${if follows != [] then "inputs:\n      ${yaml_follows}" else ""}
          '';
      in
      "To use '${attribute}', ${command}";

    getInput = args:
      inputs.${args.name} or (throw "${config.lib._mkInputError args}\n\n");

    tryGetInput = { name, url, attribute, follows ? [ ] }:
      inputs.${name} or null;

    # Like getInput but for multiple inputs at once.
    # Returns an attrset keyed by input name.
    # Throws a single error listing all missing inputs.
    getInputs = argsList:
      let
        results = map
          (args: {
            inherit args;
            value = config.lib.tryGetInput args;
          })
          argsList;
        missing = builtins.filter (r: r.value == null) results;
        missingErrors = map (r: config.lib._mkInputError r.args) missing;
        check =
          if missingErrors != [ ]
          then throw (builtins.concatStringsSep "\n\n" missingErrors)
          else null;
      in
      builtins.listToAttrs (map
        (r: {
          name = r.args.name;
          value = if r.value != null then r.value else builtins.seq check null;
        })
        results);

    # Generate a shell expression that checksums a file for change detection.
    # Returns only the checksum value (no filename).
    _fileChecksum = path: "$(${pkgs.coreutils}/bin/cksum ${lib.escapeShellArg path} | ${pkgs.coreutils}/bin/cut -f1 -d' ')";

    mkTests = folder:
      let
        mk = dir: {
          tags = [ "local" ];
          src = "${folder}/${dir}";
        };
      in
      lib.genAttrs (builtins.attrNames (builtins.readDir folder)) mk;
  };
}
