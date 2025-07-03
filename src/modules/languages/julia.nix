{ pkgs, config, lib, ... }:

let
  cfg = config.languages.julia;
in
{
  options.languages.julia = {
    enable = lib.mkEnableOption "tools for Julia development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.julia-bin;
      defaultText = lib.literalExpression "pkgs.julia-bin";
      description = "The Julia package to use.";
    };

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = ''
          Enable Julia development tools.
          
          Note: Julia development tools like LanguageServer.jl and JuliaFormatter.jl
          are typically installed via Julia's package manager (Pkg), not through nixpkgs.
          
          To install these tools, run the following in a Julia REPL:
          ```julia
          using Pkg
          Pkg.add("LanguageServer")
          Pkg.add("JuliaFormatter")
          ```
          
          For VS Code users, the Julia extension will automatically install the language server.
        '';
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ];
  };
}
