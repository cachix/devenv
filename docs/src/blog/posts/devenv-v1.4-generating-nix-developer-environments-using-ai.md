---
draft: false
date: 2025-02-13
authors:
  - domenkozar
---

# devenv 1.4: Generating Nix Developer Environments Using AI

One of the main obstacles in using Nix for development environments is mastering the language itself.
It takes time to become proficient writing Nix.

How about using AI to generate it instead:

```
$ devenv generate a Python project using Torch
• Generating devenv.nix and devenv.yaml, this should take about a minute ...
```

You can also use [devenv.new](http://devenv.new) to generate a new environment.

## Generating devenv.nix for an existing project

You can also tell devenv to create a scaffold based on your existing git source code:

```
$ devenv generate
• Generating devenv.nix and devenv.yaml, this should take about a minute ...
```

## Telemetry

To continually enhance the AI’s recommendations, we collect anonymous data on the environments generated. This feedback helps us train better models and improve accuracy.

Of course, your privacy matters—if you prefer not to participate, just add the `--disable-telemetry` flag when generating environments.
We also adhere to the [donottrack](https://consoledonottrack.com/) standard.

Domen
