{ lib, config, inputs, ... }:

{
  # freestyle
  options.lib = lib.mkOption {
    type = lib.types.attrsOf lib.types.anything;
    internal = true;
  };

  config.lib = {
    getInput = { name, url, attribute, follows ? [ ] }:
      let
        flags = lib.concatStringsSep " " (map (i: "--follows ${i}") follows);
        yaml_follows = lib.concatStringsSep "\n      " (map (i: "${i}:\n        follows: ${i}") follows);
        command =
          if config.devenv.flakesIntegration
          then ''
            Add the following to flake.nix:

            inputs.${name}.url = "${url}";
            ${if follows != [] 
              then "inputs.${name}.inputs = { ${lib.concatStringsSep "; " (map (i: "${i}.follows = \"${i}\";") follows)}; };" 
              else ""}
          ''
          else if lib.versionAtLeast config.devenv.cliVersion "1.0"
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
        inputs.${name} or (throw "To use '${attribute}', ${command}\n\n");

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
