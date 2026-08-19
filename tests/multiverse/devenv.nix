{ inputs, pkgs, multiverse, ... }:

let
  requested = {
    cmake = "3.26.4";
    bun = "0.7.0";
  };

  # A multiverse costs one fetch and one evaluation per nixpkgs revision it
  # touches, so a set of versions is resolved through the fewest revisions that
  # can serve it. Both of these land on 2023-08-01-9e1960bc196b, the revision
  # `bun` 0.7.0 already needs, so the whole shell touches a single revision.
  pinned = multiverse.pins requested;

  # The rest of the upstream API stays reachable through the raw input.
  plan = (inputs."nixpkgs-multiverse".lib.mkMultiverse {
    system = pkgs.stdenv.hostPlatform.system;
  }).pinPlan requested;
in
{
  packages = pinned;

  assertions = [
    {
      assertion = inputs."nixpkgs-multiverse" ? lib;
      message = "The raw multiverse flake must remain available through inputs.nixpkgs-multiverse.";
    }
    {
      assertion = multiverse.bun."0.7.0".version == "0.7.0";
      message = "Multiverse must expose historical Bun versions.";
    }
    {
      assertion = multiverse.cmake ? "3.16.5";
      message = "Multiverse must index historical CMake versions.";
    }
    {
      assertion = plan.revisions == 1;
      message = "Minimizing must serve both pins from a single nixpkgs revision.";
    }
    {
      assertion = map (pkg: pkg.version) pinned == [ "0.7.0" "3.26.4" ];
      message = "`multiverse.pins` must return the requested versions as packages.";
    }
    {
      assertion = (builtins.elemAt pinned 0).outPath == multiverse.bun."0.7.0".outPath;
      message = "Minimizing must keep Bun 0.7.0 on the revision the version index already selects.";
    }
  ];

  enterTest = ''
    cmake --version | grep 'cmake version 3.26.4'
    bun --version | grep '^0.7.0$'
  '';
}
