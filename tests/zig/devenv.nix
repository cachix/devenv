{
  languages.zig.enable = true;
  enterTest = ''
    zig version
    if [ -n "''${ZIG_GLOBAL_CACHE_DIR-}" ] && [ ! -d "$ZIG_GLOBAL_CACHE_DIR" ]; then
      echo "ZIG_GLOBAL_CACHE_DIR is set to non-existent path: $ZIG_GLOBAL_CACHE_DIR"
      exit 1
    fi
  '';
}
