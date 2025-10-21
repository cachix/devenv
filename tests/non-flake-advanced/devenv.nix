{
  languages.rust = {
    enable = true;
    channel = "nightly";
    targets = [ "thumbv8m.main-none-eabihf" ];
    components = [
      "rustfmt"
      "rust-analyzer"
      "miri"
    ];
  };
}
