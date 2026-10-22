{
  config,
  lib,
  pkgs,
  ...
}:
{
  languages.rust.enable = true;

  enterTest = ''
    ${lib.getExe pkgs.python3} ${./test_response_file.py} \
      ${lib.getExe pkgs.gcc} \
      ${config.env."CARGO_TARGET_${pkgs.stdenv.hostPlatform.rust.cargoEnvVarTarget}_LINKER"}
  '';
}
