# Exercises daemon pid-file/socket ownership: a second `up -d` attaches
# instead of clobbering the daemon's runtime files, a non-interactive
# foreground `up` fails fast, `down` stops the daemon without orphaning its
# children, and `down` is idempotent.
{ pkgs, ... }:
{
  packages = [ pkgs.python3 pkgs.curl ];
  process.manager.implementation = "native";
  processes.http.exec = "exec python3 -m http.server 18457";
}
