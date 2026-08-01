{ lib, pkgs, ... }:

{
  options.profilePriorityTest.package = lib.mkOption {
    type = lib.types.package;
  };

  config = {
    profilePriorityTest.package = pkgs.hello;

    profiles.package-override.module = {
      profilePriorityTest.package = pkgs.curl;
    };
  };
}
