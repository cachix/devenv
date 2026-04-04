{
  git-hooks.hooks = {
    no-op = {
      enable = true;
      name = "No Op";
      pass_filenames = false;
      raw.always_run = true;
      entry = "true";
    };
  };
}
