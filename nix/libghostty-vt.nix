{
  lib,
  stdenv,
  buildPackages,
  callPackage,
  ghostty,
}:

(callPackage "${ghostty}/nix/libghostty-vt.nix" {
  optimize = "ReleaseSafe";
  zig_0_16 = buildPackages.callPackage ./zig-binary.nix { };
}).overrideAttrs
  (old: {
    # The upstream Zig binary is built against musl. Select the library's ABI
    # explicitly instead of relying on native detection inside the sandbox.
    zigBuildFlags =
      (old.zigBuildFlags or [ ])
      ++ lib.optionals stdenv.hostPlatform.isLinux [
        "-Dtarget=${
          if stdenv.hostPlatform.isx86_32 then "x86" else stdenv.hostPlatform.parsed.cpu.name
        }-linux-${stdenv.hostPlatform.parsed.abi.name}"
      ];
  })
