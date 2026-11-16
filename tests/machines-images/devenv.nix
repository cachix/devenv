{ ... }:
{
  machines.dev = {
    systems = [
      "x86_64-linux"
      "aarch64-linux"
    ];
    nixos = {
      system.stateVersion = "26.05";
      environment.etc.shared-marker.text = "shared";
    };
    # Small artifacts exercise real CLI selection and builds without building
    # four complete operating systems in every test run. The provider builders
    # themselves remain available through system.build.amazonImage, etc.
    images.amazon = { pkgs, lib, ... }: {
      nixpkgs.overlays = [ (_final: _prev: { imageProviderMarker = "amazon"; }) ];
      environment.etc.provider-marker.text = pkgs.imageProviderMarker;
      system.build.image = lib.mkForce (
        pkgs.runCommand "test-amazon-image" { } ''
          mkdir -p $out
          echo ${pkgs.stdenv.hostPlatform.system} > $out/system
          echo amazon > $out/format
        ''
      );
    };
    images.google-compute = { pkgs, lib, ... }: {
      nixpkgs.overlays = [ (_final: _prev: { imageProviderMarker = "google-compute"; }) ];
      environment.etc.provider-marker.text = pkgs.imageProviderMarker;
      system.build.image = lib.mkForce (
        pkgs.runCommand "test-google-compute-image" { } ''
          mkdir -p $out
          echo ${pkgs.stdenv.hostPlatform.system} > $out/system
          echo google-compute > $out/format
        ''
      );
    };
  };
}
