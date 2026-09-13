{ pkgs, ... }:
{
  packages = [ pkgs.jq ];

  claude.code = {
    enable = true;

    # A `|`-separated matcher builds a watch list of several literal
    # filenames at once - not a glob alternation, just plain string
    # splitting on `|`. This watches exactly two files: ".env" and
    # ".env.local", relative to the project root.
    hooks.reload-dotenv = {
      hookType = "FileChanged";
      matcher = ".env|.env.local";
      command = "echo 'dotenv file changed' >> .claude-filechanged.log";
    };
  };
}
