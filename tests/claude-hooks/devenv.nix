{ pkgs, ... }: {
  packages = [ pkgs.jq ];

  claude.code = {
    enable = true;

    hooks = {
      # FileChanged: matcher is a glob against project-relative paths, not a
      # tool-name regex, and has no explicit timeout (uses Claude Code's default).
      reload-direnv = {
        enable = true;
        name = "Reload direnv on .envrc changes";
        hookType = "FileChanged";
        matcher = ".envrc";
        command = "direnv reload";
      };

      run-tests = {
        enable = true;
        name = "Run tests after edit";
        hookType = "PostToolUse";
        matcher = "^(Edit|MultiEdit|Write)$";
        command = "cargo test";
        timeout = 120;
      };

      log-completion = {
        enable = true;
        name = "Log when Claude finishes";
        hookType = "Stop";
        command = "echo 'Claude finished responding' >> claude.log";
      };
    };
  };
}
