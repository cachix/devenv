# Adapted from https://git.lix.systems/lix-project/flake-compat/src/branch/main/default.nix
{ src
, system ? builtins.currentSystem or "unknown-system"
,
}:

let
  lockFilePath = src + "/devenv.lock";

  lockFile = builtins.fromJSON (builtins.readFile lockFilePath);

  rootSrc = {
    lastModified = 0;
    lastModifiedDate = formatSecondsSinceEpoch 0;
    # *hacker voice*: it's definitely a store path, I promise (actually a
    # nixlang path value, likely not pointing at the store).
    outPath = src;
  };

  # Format number of seconds in the Unix epoch as %Y%m%d%H%M%S.
  formatSecondsSinceEpoch =
    t:
    let
      rem = x: y: x - x / y * y;
      days = t / 86400;
      secondsInDay = rem t 86400;
      hours = secondsInDay / 3600;
      minutes = (rem secondsInDay 3600) / 60;
      seconds = rem t 60;

      # Courtesy of https://stackoverflow.com/a/32158604.
      z = days + 719468;
      era = (if z >= 0 then z else z - 146096) / 146097;
      doe = z - era * 146097;
      yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
      y = yoe + era * 400;
      doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
      mp = (5 * doy + 2) / 153;
      d = doy - (153 * mp + 2) / 5 + 1;
      m = mp + (if mp < 10 then 3 else -9);
      y' = y + (if m <= 2 then 1 else 0);

      pad = s: if builtins.stringLength s < 2 then "0" + s else s;
    in
    "${toString y'}${pad (toString m)}${pad (toString d)}${pad (toString hours)}${pad (toString minutes)}${pad (toString seconds)}";

  allNodes = builtins.mapAttrs
    (
      key: node:
        let
          sourceInfo =
            if key == lockFile.root then
              rootSrc
            # Path inputs pointing to project root (path = ".") should use rootSrc
            # to avoid fetchTree hashing the entire project directory
            else if node.locked.type or null == "path" && node.locked.path or null == "." then
              rootSrc
            else
              let
                locked = node.locked;
                isRelativePath = p: p != null && (builtins.substring 0 2 p == "./" || builtins.substring 0 3 p == "../");
                # Resolve relative paths against src
                resolvedLocked = locked
                  // (if locked.type or null == "path" && isRelativePath (locked.path or null)
                then { path = toString src + "/${locked.path}"; }
                else { })
                  // (if locked.type or null == "git" && isRelativePath (locked.url or null)
                then { url = toString src + "/${locked.url}"; }
                else { });
              in
              builtins.fetchTree (node.info or { } // removeAttrs resolvedLocked [ "dir" ]);

          subdir = if key == lockFile.root then "" else node.locked.dir or "";

          outPath = sourceInfo + ((if subdir == "" then "" else "/") + subdir);

          # Resolve a input spec into a node name. An input spec is
          # either a node name, or a 'follows' path from the root
          # node.
          resolveInput =
            inputSpec: if builtins.isList inputSpec then getInputByPath lockFile.root inputSpec else inputSpec;

          # Follow an input path (e.g. ["dwarffs" "nixpkgs"]) from the
          # root node, returning the final node.
          getInputByPath =
            nodeName: path:
            if path == [ ] then
              nodeName
            else
              getInputByPath
                # Since this could be a 'follows' input, call resolveInput.
                (resolveInput lockFile.nodes.${nodeName}.inputs.${builtins.head path})
                (builtins.tail path);

          inputs = builtins.mapAttrs (inputName: inputSpec: allNodes.${resolveInput inputSpec}) (
            node.inputs or { }
          );

          # Only import flake.nix for non-root nodes (root doesn't need it)
          flake = if key == lockFile.root then null else import (outPath + "/flake.nix");

          outputs = if key == lockFile.root then { } else flake.outputs (inputs // { self = result; });

          # Lazy devenv evaluation for this input
          devenvEval =
            let
              bootstrapLib = import ./bootstrapLib.nix { inputs = inputs; };
            in
            bootstrapLib.mkDevenvForInput {
              input = { inherit outPath sourceInfo; };
              allInputs = inputs;
              inherit system;
            };

          result =
            outputs
            // sourceInfo
            // {
              inherit outPath;
              inherit inputs;
              inherit outputs;
              inherit sourceInfo;
              _type = "flake";
              devenv = devenvEval;
            };

          nonFlakeResult = sourceInfo // {
            inherit outPath;
            inherit inputs;
            inherit sourceInfo;
            _type = "flake";
            devenv = devenvEval;
          };

        in
        if node.flake or true && key != lockFile.root then
          assert builtins.isFunction flake.outputs;
          result
        else
          nonFlakeResult
    )
    lockFile.nodes;

  result =
    if !(builtins.pathExists lockFilePath) then
      throw "${lockFilePath} does not exist"
    else if lockFile.version >= 5 && lockFile.version <= 7 then
      allNodes.${lockFile.root}
    else
      throw "lock file '${lockFilePath}' has unsupported version ${toString lockFile.version}";

in
{
  inputs = result.inputs or { } // {
    self = result;
  };
}
