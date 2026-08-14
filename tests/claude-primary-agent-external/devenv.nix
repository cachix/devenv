{ ... }: {
  claude.code = {
    enable = true;
    # "general-purpose" is a built-in Claude Code agent. It is intentionally
    # not one of the project-defined agents below, so it should be reported
    # as the primary agent while all project agents show up as sub-agents.
    agent = "general-purpose";

    agents = {
      code-reviewer = {
        description = "Reviews code for quality, security and best practices.";
        prompt = "You are an expert code reviewer.";
      };
      docs-writer = {
        description = "Writes and maintains project documentation.";
        prompt = "You are a technical writer.";
      };
      test-writer = {
        description = "Writes comprehensive test suites.";
        prompt = "You are a test writing specialist.";
      };
    };
  };
}
