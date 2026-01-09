{ pkgs, ... }:

{
  process.manager.implementation = "overmind";
  processes.foo.exec = "echo foo; exec sleep inf";
  processes.bar.exec = "echo bar; exec sleep inf";
}
