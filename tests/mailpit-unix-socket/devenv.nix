{ config, pkgs, ... }:
let
  uiSocket = "${config.env.DEVENV_RUNTIME}/mailpit-ui.sock";
  smtpSocket = "${config.env.DEVENV_RUNTIME}/mailpit-smtp.sock";
in
{
  packages = [ pkgs.curl ];

  env = {
    MAILPIT_UI_SOCKET = uiSocket;
    MAILPIT_SMTP_SOCKET = smtpSocket;
  };

  services.mailpit = {
    enable = true;
    # The trailing 660 is the socket's permissions, not a port.
    uiListenAddress = "unix:${uiSocket}:660";
    smtpListenAddress = "unix:${smtpSocket}:660";
  };

  processes.mailpit.ready = {
    exec = "test -S ${uiSocket} && test -S ${smtpSocket}";
    initial_delay = 1;
  };
}
