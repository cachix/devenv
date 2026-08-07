{ ... }: {
  claude.code = {
    enable = true;
    # "general-purpose" is a built-in Claude Code agent, not one of the
    # (nonexistent, here) project-defined agents.
    agent = "general-purpose";
  };
}
