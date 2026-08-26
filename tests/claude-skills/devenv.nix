{ ... }:
{
  claude.code = {
    enable = true;

    skills = {
      # Left on the default copyMode on purpose: SKILL.md and its resources are
      # symlinked into the store, which is the shape most projects will get.
      package-scoping = {
        # A colon and a quote, to prove the description survives as a YAML scalar.
        description = ''Decide system vs user: use when a "package" lands in the wrong layer.'';
        allowedTools = [
          "Read"
          "Grep"
          "Bash(git status: *)"
        ];
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

      # Every field the module knows how to render, to prove each one lands in
      # the frontmatter and that a value containing a comma survives the list.
      everything = {
        description = "A skill exercising every frontmatter field.";
        whenToUse = "Use when checking that the frontmatter renders in full.";
        allowedTools = [
          "Read"
          "Bash(git log --format=a,b:*)"
        ];
        disallowedTools = [ "AskUserQuestion" ];
        disableModelInvocation = true;
        userInvocable = false;
        argumentHint = "[issue-number]";
        arguments = [
          "issue"
          "branch"
        ];
        model = "opus";
        effort = "high";
        context = "fork";
        agent = "code-reviewer";
        background = false;
        content = "Body of everything.";
      };
    };
  };
}
