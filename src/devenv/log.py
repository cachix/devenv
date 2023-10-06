from typing import Literal
import time

import click


class log_task:
    """Context manager for logging progress of a task."""

    def __init__(self, message, newline=True):
        self.message = message
        self.newline = newline

    def __enter__(self):
        prefix = click.style("•", fg="blue")
        self.start = time.time()
        click.echo(f"{prefix} {self.message} ...", nl=self.newline)

    def __exit__(self, exc, *args):
        end = time.time()
        if exc:
            prefix = click.style("✖", fg="red")
        else:
            prefix = click.style("✔", fg="green")
        click.echo(f"\r{prefix} {self.message} in {end - self.start:.1f}s.")


LogLevel = Literal["info", "warning", "error", "debug"]


def log(message, level: LogLevel):
    match level:
        case "info":
            click.echo(click.style("• ", fg="green") + message)
        case "warning":
            click.echo(click.style("• ", fg="yellow") + message, err=True)
        case "error":
            click.echo(click.style("✖ ", fg="red") + message, err=True)
        case "debug":
            click.echo(click.style("• ", fg="magenta") + message, err=True)


def log_error(message):
    log(message, "error")


def log_warning(message):
    log(message, "warning")


def log_info(message):
    log(message, "info")


def log_debug(message):
    log(message, "debug")
