{ pkgs, lib, config, ... }: {
  options = {
    myapp.package = pkgs.lib.mkOption {
      type = config.lib.types.outputOf lib.types.package;
      description = "The package for myapp1";
      default = pkgs.writeText "myapp1" "touch $out";
      defaultText = "myapp1";
    };
    myapp2.package = pkgs.lib.mkOption {
      type = config.lib.types.output;
      description = "The package for myapp2";
      default = pkgs.writeText "myapp2" "touch $out";
      defaultText = "myapp2";
    };
  };
  config = {
    enterTest = ''
      devenv build | grep -E '(myapp1|git|myapp2|ncdu)'
      devenv build myapp2.package | grep myapp2
    '';
    outputs = {
      myproject.git = pkgs.git;
      ncdu = pkgs.ncdu;
    };
  };
}
