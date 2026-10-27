{ config, ... }:
{
  env.TEST_TOKEN = config.secretspec.secrets.TEST_TOKEN;
  env.TEST_FILE = config.secretspec.secrets.TEST_FILE;
  enterShell = ''
    echo REDACTION_SHELL_READY
  '';
}
