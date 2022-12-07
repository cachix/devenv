{ pkgs, ... }:

{
  process.implementation = "hivemind";
  processes.foo.exec = "echo foo; sleep inf";
  processes.bar.exec = "echo bar; sleep inf";
}
