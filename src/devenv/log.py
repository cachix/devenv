from typing import Literal

import click


class log_task:
    """Context manager for logging progress of a task."""
    def __init__(self, message,):
        self.message = message

    def __enter__(self):
        prefix = click.style("•", fg="blue")
        click.echo(f"{prefix} {self.message} ...", nl=False)

    def __exit__(self, exc, *args):
        if exc:
            prefix = click.style("✖", fg="red")
        else:
            prefix = click.style("✔", fg="green")
        click.echo(f"\r{prefix} {self.message}")

LogLevel = Literal["info", "warning", "error"]

def log(message, level: LogLevel):
    match level:
        case "info":
            click.echo(click.style("• ", fg="green") + message)
        case "warning":
            click.echo(click.style("• ", fg="yellow") + message, err=True)
        case "error":
            click.echo(click.style("✖ ", fg="red") + message, err=True)
