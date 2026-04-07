{ pkgs, inputs, ... }:

let
  nix2container = inputs.nix2container.packages.${pkgs.stdenv.system}.nix2container;
in
{
  name = "from-image-test";

  containers.test = {
    name = "from-image-test";
    fromImage = nix2container.pullImage {
      imageName = "docker.io/library/alpine";
      imageDigest = "sha256:a8560b36e8b8210634f77d9f7f9efd7ffa463e380b75e2e74aff4511df3ef88c";
      sha256 = "sha256-hCgBDeQAulu/MSPPvojHcoynV1v1pjXtkir/dULO3Wk=";
    };
  };
}
