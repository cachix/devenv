{ pkgs, ... }:
{
  imports = [
    {
      machines.server.nixos = {
        services.openssh.ports = [ 2222 ];
      };
    }
    {
      machines.server.nixos =
        { ... }:
        {
          networking.hostName = "merged";
        };
    }
  ];

  machines.server = {
    system = pkgs.stdenv.hostPlatform.system;
    hardware.facter = null;
    nixos = {
      services.openssh.enable = true;
    };
  };
}
