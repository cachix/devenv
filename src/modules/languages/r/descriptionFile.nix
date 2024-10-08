let
  strings = import ./utils/strings.nix;
  inherit (builtins)
    concatMap
    filter
    hasAttr
    replaceStrings
    warn
    ;
  inherit (strings)
    matches
    extractWithRegex
    splitWithRegex
    ;
in
rec {
  getSection =
    section: descriptionFile:
    extractWithRegex ".*${section}:\n +([a-zA-Z0-9., (>=)\n]*)\n[A-Z].*" descriptionFile;
  extractPackages = packages: filter (matches "^[a-zA-Z0-9.]+") packages;
  extractSectionPackages = section: extractPackages (splitWithRegex "[\n, ]+" section);
  descriptionPackages =
    descriptionFile:
    concatMap (section: extractSectionPackages (getSection section descriptionFile)) [
      "Imports"
      "Depends"
      "Suggests"
    ];
  normalizePackageName = pkg: replaceStrings [ "." ] [ "_" ] pkg;
  getRPackage =
    pkg: pkgs:
    let
      pkgName = normalizePackageName pkg;
    in
    if hasAttr pkgName pkgs.rPackages then
      pkgs.rPackages.${pkgName}
    else
      warn "Package \"${pkgName}\" does not exist in nixpkgs." null;
  getRPackages =
    pkgs: descriptionFile:
    filter (pkg: pkg != null) (map (pkg: getRPackage pkg pkgs) (descriptionPackages descriptionFile));
}
