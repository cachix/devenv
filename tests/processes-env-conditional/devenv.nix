{ lib, config, ... }:

{
  env.ENABLE_PROCESS = true;

  # This pattern should not cause infinite recursion
  processes = lib.mkIf config.env.ENABLE_PROCESS {
    greet.exec = "echo hello";
  };

  enterTest = ''
    if grep -q "greet" "$PC_CONFIG_FILES"; then
      echo "PASS: 'greet' process is defined in process-compose config"
    else
      echo "FAIL: Expected 'greet' process to be defined in process-compose config"
      exit 1
    fi
  '';
}
