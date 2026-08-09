{ inputs, multiverse, ... }:

{
  packages = [
    multiverse.cmake."3.16.5"
    multiverse.bun."0.7.0"
  ];

  assertions = [
    {
      assertion = inputs."nixpkgs-multiverse" ? lib;
      message = "The raw multiverse flake must remain available through inputs.nixpkgs-multiverse.";
    }
    {
      assertion = multiverse.cmake."3.16.5".version == "3.16.5";
      message = "Multiverse must expose historical CMake versions.";
    }
    {
      assertion = multiverse.bun."0.7.0".version == "0.7.0";
      message = "Multiverse must expose historical Bun versions.";
    }
  ];

  enterTest = ''
    cmake --version | grep 'cmake version 3.16.5'
    bun --version | grep '^0.7.0$'
  '';
}
