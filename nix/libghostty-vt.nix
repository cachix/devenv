{
  lib,
  stdenv,
  callPackage,
  ghostty,
}:

(callPackage "${ghostty}/nix/libghostty-vt.nix" {
  optimize = "ReleaseSafe";
}).overrideAttrs
  (old: {
    # Select the library's Linux ABI explicitly, including for musl builds.
    zigBuildFlags =
      (old.zigBuildFlags or [ ])
      ++ lib.optionals stdenv.hostPlatform.isLinux [
        "-Dtarget=${
          if stdenv.hostPlatform.isx86_32 then "x86" else stdenv.hostPlatform.parsed.cpu.name
        }-linux-${stdenv.hostPlatform.parsed.abi.name}"
      ];
  })
