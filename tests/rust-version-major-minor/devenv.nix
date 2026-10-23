{
  # A `major.minor` version selects the newest matching stable release.
  languages.rust = {
    enable = true;
    channel = "stable";
    version = "1.89";
  };

  enterTest = ''
    rustc --version | grep -E "^rustc 1\.89\.[0-9]+"
    cargo --version | grep -E "^cargo 1\.89\.[0-9]+"
  '';
}
