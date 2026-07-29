---
title: Alternative process managers
description: Advanced integrations for external process managers.
---

:::note[Advanced configuration]

Devenv's [native process manager](/processes/) is the default, recommended
implementation and supports the complete process feature set. Most projects
should use it.

:::

If an existing workflow depends on a specific external process manager, devenv
can integrate with:

- [process-compose](./process-compose/) — feature-rich supervision with a TUI
- [overmind](./overmind/) — Procfile-based supervision with tmux integration
- [mprocs](./mprocs/) — a cross-platform TUI process runner
- [hivemind](./hivemind/) — a small Procfile process manager
- [honcho](./honcho/) — a Python Foreman port

These integrations are compatibility options. Their behavior and supported
features are determined by the external manager and may differ from the native
manager.
