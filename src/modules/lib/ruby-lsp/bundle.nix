rubyVersion:
let
  lockfile = builtins.readFile ./Gemfile.lock;
  gemset = import ./gemset.nix;

  # Nokogiri 1.19 requires Ruby 3.2 or newer.
  legacy = builtins.compareVersions rubyVersion "3.2" < 0;
  legacyNokogiri = gemset.nokogiri // {
    version = "1.18.10";
    source = gemset.nokogiri.source // {
      sha256 = "1hcwwr2h8jnqqxmf8mfb52b0dchr7pm064ingflb78wa00qhgk6m";
    };
  };

  lockedNokogiri = "    nokogiri (${gemset.nokogiri.version})";
  legacyLockfile =
    assert builtins.elem lockedNokogiri (builtins.split "\n" lockfile)
      || throw "Ruby LSP Gemfile.lock is missing Nokogiri ${gemset.nokogiri.version}. Run bundix in src/modules/lib/ruby-lsp to sync gemset.nix.";
    builtins.replaceStrings [ "\n${lockedNokogiri}\n" ] [ "\n    nokogiri (${legacyNokogiri.version})\n" ] lockfile;
in
{
  inherit legacy;
  gemset = if legacy then gemset // { nokogiri = legacyNokogiri; } else gemset;
  lockfile = if legacy then legacyLockfile else lockfile;
}
