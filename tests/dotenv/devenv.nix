{ pkgs, ... }: {
  dotenv.enable = true;
  dotenv.filename = [ ".env" "config/.env.bar" "generated/.env" ];
  dotenv.substitution = true;

  env.BAR = "1";
  env.DISABLED = null;

  # This file is created by devenv:files immediately before devenv:enterShell.
  files."generated/.env".text = "TASK_GENERATED=yes\n";

  tasks."test:mutate-dotenv" = {
    exec = ''
      sed -i 's/MUTATED_BY_TASK=before/MUTATED_BY_TASK=after/' .env
      sed -i '/^REMOVED_BY_TASK=/d' .env
    '';
    before = [ "devenv:enterShell" ];
  };
}
