{ config, lib, ... }:
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

  config.changelogs = [
    {
      date = "2026-11-02";
      title = "Configure shared proxy listen addresses in devenv.nix";
      when = config.process.proxy.enable;
      description = ''
        Set `process.proxy.listen` and `process.proxy.https.listen` in `devenv.nix`
        to share proxy addresses across the team without per-developer shell variables.
        For example, `[::1]:1080` for HTTP and `[::1]:1443` for HTTPS avoid Linux
        low-port sudo prompts and support clients that resolve `.localhost` to IPv6.
        These addresses bind IPv6 only; use `127.0.0.1` for IPv4 clients.
        The defaults are unchanged, and `DEVENV_PROXY_LISTEN` and
        `DEVENV_PROXY_HTTPS_LISTEN` in the CLI environment still take precedence.
        If a running shared proxy uses different addresses or is too old to report
        its HTTP address, stop it using the PID shown in the error and run `devenv up`
        again. Stopping the shared proxy interrupts proxy access for other projects.
      '';
    }
  ];
}
