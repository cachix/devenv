# Resolves the devenv-tasks binary using devenv's own locked nixpkgs,
# regardless of the user's nixpkgs. This ensures the store path matches
# what CI builds and pushes to devenv.cachix.org.
#
# pkgs is the user's nixpkgs — only used for fixed-output fetches
# (fetchFromGitHub), so the result is independent of the user's nixpkgs version.
{ pkgs, lib }:
let
  lock = builtins.fromJSON (builtins.readFile ./../../../flake.lock);
  nixpkgsNodeName = lock.nodes.root.inputs.nixpkgs;
  lockedNixpkgs = lock.nodes.${nixpkgsNodeName}.locked;
  lockedRustOverlay = lock.nodes.rust-overlay.locked or null;

  rustOverlaySource =
    if lockedRustOverlay != null && lockedRustOverlay.type == "github" then
      pkgs.fetchFromGitHub
        {
          inherit (lockedRustOverlay) owner repo rev;
          hash = lockedRustOverlay.narHash;
        }
    else
      null;

  devenvPkgs =
    if lockedNixpkgs.type == "github" then
      let
        source = pkgs.fetchFromGitHub {
          inherit (lockedNixpkgs) owner repo rev;
          hash = lockedNixpkgs.narHash;
        };
        rustOverlay = if rustOverlaySource != null then import rustOverlaySource else null;
        overlays = lib.optional (rustOverlay != null) rustOverlay;
      in
      import source { inherit overlays; system = pkgs.stdenv.system; }
    else
      builtins.trace
        "warning: devenv-tasks: could not resolve devenv's nixpkgs from flake.lock (type=${lockedNixpkgs.type}); falling back to user's nixpkgs. The resulting store path may not match the cached binary."
        pkgs;

  rustToolchain =
    if devenvPkgs ? rust-bin
    then devenvPkgs.rust-bin.stable.latest.default
    else devenvPkgs.rustc;

  workspace = devenvPkgs.callPackage ./../../../nix/workspace.nix {
    rustc = rustToolchain;
    cargo = rustToolchain;
  };
in
workspace.crates.devenv-tasks
