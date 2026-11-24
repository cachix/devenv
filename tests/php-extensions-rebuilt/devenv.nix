{
  pkgs,
  config,
  lib,
  ...
}:

let
  # languages.php.package rebuilt as another variant: the thread-safe PHP with the embed SAPI that
  # nixpkgs' FrankenPHP builds from it (`passthru.php`; FrankenPHP itself is not built). Nix runs
  # the package's extensions function again for the new PHP, so languages.php.extensions have to
  # come from its `all` argument: extensions of the original package are built for the original
  # PHP, which the rebuilt one cannot load.
  rebuilt = (pkgs.frankenphp.override { php = config.languages.php.package; }).php;
in
{
  languages.php = {
    enable = true;
    # Not enabled by default, so it comes from languages.php.extensions.
    extensions = [ "shmop" ];
  };

  env.REBUILT_PHP = lib.getExe rebuilt;
}
