{ pkgs, lib, pre-commit, ... } :

{
  options.pre-commit = {
    enable = lib.mkEanble "pre-commit";

    # TODO: look at flakes-parts?

  };

  config = {

  };
}