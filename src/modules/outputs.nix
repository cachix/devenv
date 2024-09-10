{ pkgs, lib, config, ... }: {
  options = {
    outputs = lib.mkOption {
      type = config.lib.types.outputOf lib.types.attrs;
      default = {
        git = pkgs.git;
        foo = {
          ncdu = pkgs.ncdu;
        };
      };
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
      check = t.check;
      merge = t.merge;
      emptyValue = t.emptyValue;
      getSubOptions = t.getSubOptions;
      getSubModules = t.getSubModules;
      substSubModules = t.substSubModules;
      nestedTypes.elemType = t;
    };
  };
}
