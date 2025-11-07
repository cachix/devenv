{ lib, config, ... }: {
  options = {
    outputs = lib.mkOption {
      type = config.lib.types.outputOf lib.types.attrs;
      default = { };
      example = lib.literalExpression ''
        {
          git = pkgs.git;
          foo = {
            ncdu = pkgs.ncdu;
          };
        }
      '';
      description = ''
        Nix outputs for `devenv build` consumption.
      '';
    };
  };

  config.lib.types = {
    output = lib.types.anything // {
      name = "output";
      description = "output";
      descriptionClass = "output";
    };

    outputOf = t: lib.types.mkOptionType {
      name = "outputOf";
      description = "outputOf ${lib.types.optionDescriptionPhrase (class: class == "noun" || class == "conjunction") t}";
      descriptionClass = "outputOf";
      inherit (t) check merge emptyValue getSubOptions getSubModules substSubModules;
      nestedTypes.elemType = t;
    };
  };
}
