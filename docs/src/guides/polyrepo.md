# Polyrepos

When working with multiple projects across separate repositories, you may want
to compose environments or reference options --- such as outputs, packages, or
other configuration --- defined in one devenv project from another.

There are two approaches:

- **[Composing with imports](#composing-with-imports)** --- merge an entire project's configuration (packages, services, env, etc.) into your environment.
- **[Referencing config across inputs](#referencing-config-across-inputs)** --- access specific config from another project without merging everything.

!!! warning
    The remote repository must use `devenv.nix` only --- `devenv.yaml` from
    imported projects is not evaluated. See
    [#2205](https://github.com/cachix/devenv/issues/2205) for details.

Both examples below reference the same remote repository (`myorg/my-service`)
with the following configuration:

```nix title="my-service/devenv.nix"
{ config, ... }: {
  languages.python.enable = true;

  outputs.my-service = config.languages.python.import ./. {};

  processes.my-service.exec = "${config.outputs.my-service}/bin/my-service";
}
```

## Composing with imports

devenv projects compose naturally through imports. When you import another
project via an input, all of its configuration --- packages, services, outputs,
env, and more --- merges into your environment.

Add the remote repository as an input, then import from it:

```yaml title="devenv.yaml"
inputs:
  my-service:
    url: github:myorg/my-service
imports:
  - my-service
```

Any configuration defined in the imported project's `devenv.nix` merges into
your environment. For example, `my-service`'s output and process are now
available via `config`:

```nix title="devenv.nix"
{ config, ... }: {
  # my-service's output is merged into config.outputs
  packages = [ config.outputs.my-service ];

  # my-service's process is also merged, so `devenv up` will start it
}
```

For local cross-project imports (monorepos), see the
[monorepo guide](monorepo.md).

## Referencing config across inputs

!!! info "New in version 2.0"

When you don't want to merge an entire environment but need access to specific
options from another project, you can reference them through
`inputs.<name>.devenv.config`. This is particularly useful for
consuming [outputs](../outputs.md) defined in other projects.

```yaml title="devenv.yaml"
inputs:
  my-service:
    url: github:myorg/my-service
    flake: false
```

```nix title="devenv.nix"
{ inputs, ... }: {
  packages = [
    inputs.my-service.devenv.config.outputs.my-service
  ];

  processes.my-service.exec = "${inputs.my-service.devenv.config.outputs.my-service}/bin/my-service";
}
```

!!! warning
    Profiles don't work with cross-project references. See
    [#2521](https://github.com/cachix/devenv/issues/2521) for details.

