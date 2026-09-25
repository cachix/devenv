{ ... }:
{
  services.mysql = {
    enable = true;
    initialDatabases = [ { name = "named_up"; } ];
    ensureUsers = [
      {
        name = "named_up";
        password = "named_up";
        ensurePermissions."named_up.*" = "ALL PRIVILEGES";
      }
    ];
  };
}
