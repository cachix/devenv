{ pkgs, ... }:

{
  procfileScript = ''
    OVERMIND_ENV=$procfileenv ${pkgs.overmind}/bin/overmind start --procfile "$procfile"
  '';
  processes.foo.exec = "echo foo; sleep inf";
  processes.bar.exec = "echo bar; sleep inf";
}
