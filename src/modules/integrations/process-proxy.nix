{ lib, ... }:
{
  options.process.proxy.enable = lib.mkEnableOption ''
    the shared HTTP proxy for friendly process URLs under ``.localhost``
  '' // {
    default = true;
  };
}
