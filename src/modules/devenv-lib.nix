{ pkgs, lib, config, inputs }:

{
  getInput = { name, url, attribute, follows ? [ ] }:
    let
      flags = lib.concatStringsSep " " (map (i: "--follows ${i}") follows);
      yaml_follows = lib.concatStringsSep "\n      " (map (i: "${i}:\n        follows: ${i}") follows);
      command =
        if lib.versionAtLeast config.devenv.cliVersion "1.0"
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
}
