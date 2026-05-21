{ pkgs, ... }: {
  # python3 used by .test.sh for port discovery
  packages = [ pkgs.python3 ];
}
