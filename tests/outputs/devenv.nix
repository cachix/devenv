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
    outputs = {
      myproject.git = pkgs.git;
      hello = pkgs.hello;
    };
  };
}
