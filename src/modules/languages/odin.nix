{ pkgs, config, lib, ... }:

let
  cfg = config.languages.odin;
in
{
  options.languages.odin = {
    enable = lib.mkEnableOption "tools for Odin Language";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.odin;
      defaultText = lib.literalExpression "pkgs.odin";
      description = "The odin package to use.";
    };

    debugger = lib.mkOption {
      type = lib.types.package;
      default = pkgs.gdb;
      defaultText = lib.literalExpression "pkgs.gdb";
      description = "The debugger package to use with odin.";
    };

    doc = lib.mkOption
      {
        type = lib.types.lines;
        description = "Documentation for the Odin package.";
        default = "No documentation available";
      };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      nasm
      clang
      gnumake
      ols
      cfg.debugger
      cfg.package
    ];

    languages.odin.doc = ''
      Odin is a systems programming language designed for performance, concurrency, and simplicity.
        This package provides the Odin compiler, standard library, and associated tools for developing
        and building Odin programs.

        Key features:
        - Low-level control over data layout and memory management
        - Built-in support for concurrency with lightweight threads and message passing
        - Simple and readable syntax, inspired by C and Go
        - Powerful metaprogramming capabilities with compile-time code execution
        - Interoperability with C, allowing integration with existing codebases
    '';
  };
}
