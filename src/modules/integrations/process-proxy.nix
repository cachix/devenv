{ lib, ... }:
{
  options.process.proxy = {
    enable = lib.mkEnableOption ''
      the shared HTTP proxy for friendly process URLs under ``.localhost``
    '';

    listen = lib.mkOption {
      type = lib.types.str;
      default = "127.0.0.1:80";
      example = "[::1]:1080";
      description = ''
        HTTP socket address for the shared localhost proxy, as an IP address
        and port. IPv6 addresses must be bracketed, for example `[::1]:1080`.
        Ports above 1023 do not require Linux low-port capabilities.
        `DEVENV_PROXY_LISTEN` in the environment of the devenv CLI takes precedence.
        Projects using the shared proxy must agree on its listen address;
        changing it requires stopping the existing shared proxy.
      '';
    };

    https.listen = lib.mkOption {
      type = lib.types.str;
      default = "127.0.0.1:443";
      example = "[::1]:1443";
      description = ''
        HTTPS socket address for the shared localhost proxy, as an IP address
        and port. IPv6 addresses must be bracketed, for example `[::1]:1443`.
        `DEVENV_PROXY_HTTPS_LISTEN` in the environment of the devenv CLI takes precedence.
        This does not enable HTTPS: opt in with `processes.<name>.proxy.https.enable`.
        Projects requiring HTTPS must agree on its listen address;
        changing it requires stopping the existing shared proxy.
      '';
    };
  };
}
