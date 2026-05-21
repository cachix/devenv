{
  name = "devenv-shell-enter";

  requires = caps: caps.hasProject;

  script = ''
    out = machine.succeed(
        "su - dev -c 'cd ~/project && devenv shell -- echo INSIDE_SHELL'"
    )
    assert "INSIDE_SHELL" in out, f"shell did not run command, got: {out!r}"
    assert "DEVENV_ENTER_OK" in out, f"enterShell did not run, got: {out!r}"
  '';
}
