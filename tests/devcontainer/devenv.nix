{ pkgs, ... }:
{
  devcontainer.enable = true;
  devcontainer.settings = {
    image = "DEVCONTAINER_IMAGE_PLACEHOLDER";
  };
}