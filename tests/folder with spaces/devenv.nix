{
  cachix.enable = false;
  languages.rust.enable = true;

  enterTest = "rustc main.rs";
}
