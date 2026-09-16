{ pkgs, inputs, ... }:
{
  packages = [ pkgs.python3 ];
  env.EXECUTOR_SOURCE = "${inputs.devenv.modules}/machines/deploy.py";
}
