# Source resolution shared by YAML discovery and full flake evaluation.
{
  src,
  lockFile,
  rootSrc ? {
    outPath = src;
  },
}:
let
  sourceInfo =
    key: node:
    if key == lockFile.root then
      rootSrc
    # Path inputs pointing to project root (path = ".") should use rootSrc
    # to avoid fetchTree hashing the entire project directory
    else if node.locked.type or null == "path" && node.locked.path or null == "." then
      rootSrc
    else
      let
        locked = node.locked;
        isRelativePath =
          p: p != null && (builtins.substring 0 2 p == "./" || builtins.substring 0 3 p == "../");
        # Resolve relative paths against src
        resolvedLocked =
          locked
          // (
            if locked.type or null == "path" && isRelativePath (locked.path or null) then
              { path = toString src + "/${locked.path}"; }
            else
              { }
          )
          // (
            if locked.type or null == "git" && isRelativePath (locked.url or null) then
              { url = toString src + "/${locked.url}"; }
            else
              { }
          );

        # Local path inputs without a pinned narHash resolve to the live
        # filesystem path instead of a fetchTree store copy. The lock file
        # never carries a narHash for them (Nix strips volatile attributes
        # from local inputs), so a store copy would hide edits from the
        # eval cache: nothing in the cache key changes when the directory
        # content changes. Evaluating from the live path makes the eval
        # cache track every file that is read, so edits invalidate it.
        isLivePath =
          locked.type or null == "path"
          && !(locked ? narHash)
          && builtins.substring 0 1 (resolvedLocked.path or "") == "/";

        sourceAttrs = node.info or { } // removeAttrs resolvedLocked [ "dir" ];
        # Final fetches trust the lock's metadata and can substitute the
        # source from a binary cache. Keep ordinary fetches for inputs
        # whose missing metadata still needs to be supplied by the fetcher.
        hasLockedMetadata =
          sourceAttrs ? narHash
          && (
            sourceAttrs.type == "file"
            || (
              builtins.elem sourceAttrs.type [
                "git"
                "github"
                "gitlab"
                "sourcehut"
                "tarball"
              ]
              && sourceAttrs ? lastModified
              && (sourceAttrs.type == "tarball" || sourceAttrs ? rev)
              && (sourceAttrs.type != "git" || (sourceAttrs.shallow or true) || sourceAttrs ? revCount)
            )
          );
        fetchTree =
          if hasLockedMetadata then builtins.fetchFinalTree or builtins.fetchTree else builtins.fetchTree;
        fetchedSource = fetchTree sourceAttrs;
      in
      if isLivePath then
        {
          # Keep fetchTree's public metadata available to flakes that use
          # self.narHash or self.lastModified. These inherited values stay
          # lazy, so the live input is only hashed if metadata is read.
          inherit (fetchedSource) lastModified lastModifiedDate narHash;
          outPath = /. + resolvedLocked.path;
        }
      else
        fetchedSource;

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

  nodeSource =
    key:
    let
      node = lockFile.nodes.${key};
    in
    {
      repository_root = (sourceInfo key node).outPath;
      dir = if key == lockFile.root then "" else node.locked.dir or "";
    };
in
{
  inherit sourceInfo resolveInput nodeSource;
  nodePath =
    key:
    let
      source = nodeSource key;
    in
    source.repository_root + (if source.dir == "" then "" else "/${source.dir}");
}
