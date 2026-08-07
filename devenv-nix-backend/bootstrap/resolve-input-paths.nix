{ src, names, lockFileJSON }:

let
  lockFilePath = src + "/devenv.lock";
  lockFile = builtins.fromJSON lockFileJSON;

  resolveInput =
    inputSpec:
    if builtins.isList inputSpec then getInputByPath lockFile.root inputSpec else inputSpec;

  getInputByPath =
    nodeName: path:
    if path == [ ] then
      nodeName
    else
      getInputByPath
        (resolveInput lockFile.nodes.${nodeName}.inputs.${builtins.head path})
        (builtins.tail path);

  sourcePath =
    key: node:
    if key == lockFile.root
      || (node.locked.type or null == "path" && node.locked.path or null == ".")
    then
      src
    else
      let
        locked = node.locked;
        isRelativePath =
          path:
          path != null
          && (builtins.substring 0 2 path == "./" || builtins.substring 0 3 path == "../");
        resolvedLocked = locked
          // (if locked.type or null == "path" && isRelativePath (locked.path or null) then {
          path = src + "/${locked.path}";
        } else { })
          // (if locked.type or null == "git" && isRelativePath (locked.url or null) then {
          url = src + "/${locked.url}";
        } else { });
        fetchedSource = builtins.fetchTree (node.info or { } // removeAttrs resolvedLocked [ "dir" ]);
        isLivePath =
          locked.type or null == "path"
          && !(locked ? narHash)
          && builtins.substring 0 1 (resolvedLocked.path or "") == "/";
      in
      if isLivePath then resolvedLocked.path else fetchedSource.outPath;

  nodePath =
    key:
    let
      node = lockFile.nodes.${key};
      subdir = node.locked.dir or "";
    in
    sourcePath key node + (if subdir == "" then "" else "/${subdir}");

  rootInputs = lockFile.nodes.${lockFile.root}.inputs or { };
in
if lockFile.version < 5 || lockFile.version > 7 then
  throw "lock file '${lockFilePath}' has unsupported version ${toString lockFile.version}"
else
  builtins.listToAttrs (map
    (name:
    if !(builtins.hasAttr name rootInputs) then
      throw "input '${name}' is missing from ${lockFilePath}"
    else {
      inherit name;
      value = nodePath (resolveInput rootInputs.${name});
    })
    names)
