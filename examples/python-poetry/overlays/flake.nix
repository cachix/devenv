{
  outputs = { ... }: {
    overlays.default = self: super: {
      python311 = super.python311.override {
        packageOverrides = pyself: pysuper: {
          click = pysuper.click.overrideAttrs (_: {
            # doesnt work on ZFS (mcdonc)
            disabledTests = [ "test_file_surrogates" ];
          });
        };
      };
    };
  };
}

