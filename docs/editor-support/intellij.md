## IDE Integration for PHP Development

When developing PHP applications with Intellij IDEA or PHPStorm, install the [Devenv.sh Support plugin](https://plugins.jetbrains.com/plugin/23136-devenv-sh-support) to enable seamless integration with your devenv environment. This plugin provides first-class PHP support and automatically configures your IDE to work with the devenv-managed PHP interpreter and dependencies.

## Python Environment Configuration

For Python projects, most IDEs require direct access to the virtual environment to provide features like intelligent code completion, debugging, and package management. Since devenv stores the Python virtual environment in `.devenv/state/venv/`, you can improve IDE compatibility by creating a symbolic link in your project root.

Add this configuration to your `devenv.nix` file:

```nix
{
  enterShell = ''
    # Create a symlink to the Python virtual environment for IDE compatibility
    if [ ! -L "$DEVENV_ROOT/venv" ]; then
        ln -s "$DEVENV_STATE/venv/" "$DEVENV_ROOT/venv"
    fi
  '';
}
```

This shell hook automatically creates a `venv` symlink in your project directory when you enter the devenv shell. The symlink points to the actual virtual environment location, allowing your IDE to automatically detect and configure the Python interpreter, installed packages, and development tools.

The conditional check ensures the symlink is only created once, preventing errors on subsequent shell entries.
