{
  lib,
  pkgs,
  options,
  ...
}:
{
  assertions = [
    {
      assertion = import ./eval.nix {
        inherit lib pkgs;
        moduleDirectory = builtins.dirOf (builtins.head options.languages.javascript.enable.declarations);
      };
      message = "JavaScript/TypeScript tooling regression assertions failed";
    }
  ];
}
