{ lib
, defaults ? {
    maxFileSize = 1024 * 1024;
    maxFiles = 2;
  }
,
}:
{
  maxFileSize = lib.mkOption {
    type = lib.types.ints.positive;
    default = defaults.maxFileSize;
    description = "Maximum bytes per retained stdout/stderr log file. Only applies to the native process manager.";
  };
  maxFiles = lib.mkOption {
    type = lib.types.ints.positive;
    default = defaults.maxFiles;
    description = "Number of retained files per stream, including the active file. Only applies to the native process manager.";
  };
}
