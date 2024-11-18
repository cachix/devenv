{ pkgs, ... }:

{
  process.manager.implementation = "overmind";
  processes.foo.exec = "echo foo; sleep inf";
  processes.bar.exec = "echo bar; sleep inf";
}
