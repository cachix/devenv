{ lib, options, ... }:
{
  assertions = [
    {
      assertion = import ./eval.nix {
        inherit lib;
        module = builtins.head options.processes.declarations;
      };
      message = "Process log retention defaults, inheritance, validation, or task JSON wiring regressed";
    }
  ];
}
