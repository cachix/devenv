{
  lib,
  pkgs,
  options,
  ...
}:
{
  imports = [ ./collector.nix ];
  assertions = [
    {
      assertion = import ./eval.nix {
        inherit lib pkgs;
        module = builtins.head options.services.opentelemetry-collector.enable.declarations;
      };
      message = "OpenTelemetry Collector port allocation regression assertions failed";
    }
  ];
}
