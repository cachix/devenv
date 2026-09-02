{ pkgs, ... }: {
  packages = [ pkgs.jq ];

  claude.code = {
    enable = true;

    hooks = {
      # FileChanged: matcher is a literal filename (not a glob) relative to
      # the project root, not a tool-name regex, and has no explicit timeout
      # (uses Claude Code's default). See examples/claude-filechanged-* for
      # the single-file, "|"-separated, and filtering-matcher cases.
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
