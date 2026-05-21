{
  name = "devenv-version";

  requires = _: true;

  script = ''
    machine.succeed("su - dev -c 'devenv --version'")
  '';
}
