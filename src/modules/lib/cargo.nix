{ lib }:

# Read Cargo project files (`Cargo.toml`, `Cargo.lock`) at evaluation time.
# Exposed to users as `languages.rust.cargo.*`.
#
# Every function takes a `path`: either a directory, or the file itself
# (`./Cargo.toml`, `./Cargo.lock`). Files are read directly from the project
# tree, so devenv's evaluation cache tracks them and edits invalidate it.
let
  # Whether `path` names a file rather than a directory. Paths that do not exist
  # yet are still accepted here so the caller can report them.
  isFile = path:
    let name = baseNameOf (toString path);
    in lib.hasSuffix ".toml" name || lib.hasSuffix ".lock" name;

  projectDir = path: if isFile path then dirOf path else path;

  # Nearest directory, starting at `dir` and walking towards the filesystem
  # root, for which `pred` holds. `null` when none does.
  findUp = pred: dir:
    if pred dir then dir
    else if dirOf dir == dir then null
    else findUp pred (dirOf dir);

  readTOML = fn: file:
    if builtins.pathExists file
    then lib.importTOML file
    else throw "languages.rust.cargo.${fn}: ${toString file} does not exist";

  manifestPath = path: if isFile path then path else path + "/Cargo.toml";

  readManifest = path: readTOML "readManifest" (manifestPath path);

  hasWorkspace = dir:
    let file = dir + "/Cargo.toml";
    in builtins.pathExists file && (lib.importTOML file) ? workspace;

  # Directory containing the workspace root `Cargo.toml` for the package in
  # `dir`, found the way Cargo does: `package.workspace` when set, otherwise the
  # nearest manifest with a `[workspace]` table in `dir` or its parents.
  findWorkspaceRoot = dir:
    let
      manifest = readManifest (dir + "/Cargo.toml");
      explicit = manifest.package.workspace or null;
    in
    if manifest ? workspace then dir
    else if explicit != null then dir + "/${explicit}"
    else findUp hasWorkspace (dirOf dir);

  # `Cargo.lock` lives at the workspace root, so walk up from `path` to find it.
  lockPath = path:
    if isFile path then path
    else
      let dir = findUp (d: builtins.pathExists (d + "/Cargo.lock")) path;
      in
      if dir == null
      then throw "languages.rust.cargo.readLock: no Cargo.lock found in ${toString path} or any parent directory"
      else dir + "/Cargo.lock";

  readLock = path: readTOML "readLock" (lockPath path);

  # Cargo accepts two- or three-component versions, optionally prefixed with
  # `^`. rust-overlay only knows full versions, so `1.70` becomes `1.70.0`.
  normalizeRustVersion = version:
    let parts = lib.splitVersion (lib.removePrefix "^" version);
    in lib.concatStringsSep "." (parts ++ lib.genList (_: "0") (lib.max 0 (3 - lib.length parts)));

  # The minimum supported Rust version (`rust-version`) of the package or
  # workspace at `path`. Members inheriting it with
  # `rust-version.workspace = true` are resolved through their workspace root.
  rustVersion = path:
    let
      file = manifestPath path;
      dir = dirOf file;
      manifest = readTOML "rustVersion" file;
      packageVersion = manifest.package.rust-version or null;
      workspaceVersion = manifest.workspace.package.rust-version or null;

      inherited =
        let
          root = findWorkspaceRoot dir;
          rootFile = root + "/Cargo.toml";
        in
        if root == null
        then throw "languages.rust.cargo.rustVersion: ${toString file} inherits `rust-version` from its workspace, but no workspace root was found in any parent directory"
        else (readManifest rootFile).workspace.package.rust-version
          or (throw "languages.rust.cargo.rustVersion: ${toString file} inherits `rust-version` from its workspace, but ${toString rootFile} does not set `workspace.package.rust-version`");

      version =
        if builtins.isString packageVersion then packageVersion
        else if packageVersion == { workspace = true; } then inherited
        else if builtins.isString workspaceVersion then workspaceVersion
        else throw "languages.rust.cargo.rustVersion: no `rust-version` set in ${toString file} (checked `package.rust-version` and `workspace.package.rust-version`)";
    in
    normalizeRustVersion version;

  # The version a crate is locked at in the `Cargo.lock` found from `path`.
  lockedVersion = path: name:
    let
      file = lockPath path;
      packages = lib.filter (p: p.name == name) ((readTOML "lockedVersion" file).package or [ ]);
      versions = map (p: p.version) packages;
    in
    if packages == [ ]
    then throw "languages.rust.cargo.lockedVersion: `${name}` is not in ${toString file}"
    else if lib.length packages > 1
    then throw "languages.rust.cargo.lockedVersion: `${name}` is locked at several versions in ${toString file} (${lib.concatStringsSep ", " versions}); select one from `(languages.rust.cargo.readLock <path>).package` instead"
    else (lib.head packages).version;
in
{
  inherit readManifest readLock rustVersion lockedVersion;
}
