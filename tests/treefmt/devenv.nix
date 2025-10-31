{
  treefmt = {
    enable = true;

    config.programs = {
      nixfmt.enable = true;
    };
  };

  enterTest = ''
    treefmt
  '';
}
