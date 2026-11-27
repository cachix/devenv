let
  bundleFor = import ./bundle.nix;

  # Bundler indents gem specifications by four spaces and their dependencies by six.
  lockedVersions = lockfile:
    let
      lines = builtins.filter builtins.isString (builtins.split "\n" lockfile);
      matches = builtins.filter (entry: entry != null) (
        map (builtins.match "    ([^ ]+) \\(([^ )]+)\\)") lines
      );
      versions = builtins.listToAttrs (map
        (entry: {
          name = builtins.elemAt entry 0;
          value = builtins.elemAt entry 1;
        })
        matches);
    in
    assert builtins.length matches == builtins.length (builtins.attrNames versions)
      || throw "Ruby LSP Gemfile.lock must list one version per gem.";
    versions;

  check = rubyVersion:
    let
      bundle = bundleFor rubyVersion;
      locked = lockedVersions bundle.lockfile;
      pinned = builtins.mapAttrs (_: gem: gem.version) bundle.gemset;
      names = builtins.attrNames (locked // pinned);
      differences = builtins.filter (name: (locked.${name} or null) != (pinned.${name} or null)) names;
      describe = name: "  ${name} (${locked.${name} or "missing"} in Gemfile.lock, ${pinned.${name} or "missing"} in gemset.nix)";
    in
    if differences == [ ] then true else
    throw ''
      Ruby LSP gem versions differ for Ruby ${rubyVersion}
      ${builtins.concatStringsSep "\n" (map describe differences)}
      Run bundix in src/modules/lib/ruby-lsp to sync gemset.nix.
    '';
in
check "3.4" && check "3.1"
