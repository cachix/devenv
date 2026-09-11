{ pkgs, ... }:
{
  packages = [ pkgs.jq ];

  claude.code = {
    enable = true;

    hooks = {
      # Broad watcher: fires an "environment changed" notice for any of the
      # three env files.
      notify-env-change = {
        hookType = "FileChanged";
        matcher = ".env|.env.local|.env.production";
        command = "echo 'environment file changed' >> .claude-filechanged.log";
      };

      # Narrow watcher: fires an extra safeguard command, but only for the
      # production env file specifically.
      warn-production-env = {
        hookType = "FileChanged";
        matcher = ".env.production";
        command = "echo 'PRODUCTION env file changed - double check this!' >> .claude-filechanged.log";
      };
    };
  };
}
