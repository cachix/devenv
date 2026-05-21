{
  name = "shell-startup-clean";

  requires = _: true;

  script = ''
    out = machine.succeed("su - dev -c 'echo READY'").strip()
    assert out == "READY", f"expected clean stdout, got: {out!r}"
  '';
}
