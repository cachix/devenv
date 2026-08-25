{ ... }: {
  claude.code = {
    enable = true;

    skills = {
      # Left on the default copyMode on purpose: SKILL.md and its resources are
      # symlinked into the store, which is the shape most projects will get.
      package-scoping = {
        # A colon and a quote, to prove the description survives as a YAML scalar.
        description = ''Decide system vs user: use when a "package" lands in the wrong layer.'';
        allowedTools = [ "Read" "Grep" "Bash(git status: *)" ];
        resources = {
          "references/table.md" = ./table.md;
          "scripts/check.sh" = {
            source = ./check.sh;
            executable = true;
          };
        };
        content = ''
          # Package scoping

          Ask who needs the binary, not where you would prefer it.
        '';
      };

      minimal = {
        description = "A skill with no tool restriction and no bundled resources.";
        content = "Body.";
      };
    };
  };
}
