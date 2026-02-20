{ pkgs, ... }:

{
  # A task that always fails, used as a @complete (soft) dependency.
  # Its failure should NOT cause a non-zero exit code because it is
  # only depended on via @complete edges.
  tasks."test:will-fail" = {
    description = "Task that fails intentionally";
    exec = ''
      echo "This task is failing..."
      exit 1
    '';
  };

  # Override the default enterShell dependency to use @complete
  # so that the failure of test:will-fail does not propagate.
  tasks."devenv:enterShell".after = [ "test:will-fail@complete" ];
}
