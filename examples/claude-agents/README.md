# Claude Code Agents

Here we demonstrate a `devenv` setting with a number of agents suitable for running a spec-driven example based on [acai.sh](https://acai.sh/). Acai recommends [four types of agents](https://acai.sh/agents#factory-agent-roles):

- [supervise.md](https://acai.sh/agents#agents-supervise-md)
- [prepare-tasks.md](https://acai.sh/agents#agents-prepare-tasks-md)
- [implement.md](https://acai.sh/agents#agents-implement-md)
- [review-task.md](https://acai.sh/agents#agents-review-task-md)

Those agents are mapped in [devenv.nix](./examples/claude-agents/devenv.nix) itself. The `supervise` agent is assigned to the `agent` option, making it the primary agent.

## Feature

The feature is specified in [generic-agent.feature.yaml](features/claude-agents-example/generic-agent.feature.yaml). To get Claude to implement the feature, just say:

> I made changes to the spec - @generic-agent.feature.yaml - please run `acai skill` to learn spec-driven development.

The feature itself is already implemented -- it's the `assist` agent defined in [devenv.nix](./examples/claude-agents/devenv.nix).
