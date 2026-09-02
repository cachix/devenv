{ pkgs, ... }:
{
  packages = [ pkgs.jq ];

  claude.code = {
    enable = true;

    # `FileChanged` matchers are literal filenames, not globs or regexes.
    # This watches exactly one file, named exactly ".envrc", relative to the
    # project root - nothing else.
    hooks.reload-direnv = {
      hookType = "FileChanged";
      matcher = ".envrc";
      command = "direnv reload";
    };
  };
}
