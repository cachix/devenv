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
    system = "x86_64-linux";
    hardware.facter = null;
    nixos = {
      services.openssh.enable = true;
    };
  };
}
