{
  stdenv,
  fetchurl,
  binutils,
  zig_0_16,
}:

let
  version = "0.16.0";
  binaries = {
    x86_64-linux = {
      target = "x86_64-linux";
      sha256 = "70e49664a74374b48b51e6f3fdfbf437f6395d42509050588bd49abe52ba3d00";
    };
    aarch64-linux = {
      target = "aarch64-linux";
      sha256 = "ea4b09bfb22ec6f6c6ceac57ab63efb6b46e17ab08d21f69f3a48b38e1534f17";
    };
    i686-linux = {
      target = "x86-linux";
      sha256 = "4e34e279a9f856358de420490b531974c3d37f8f3707eef9f0342e92c14c301f";
    };
  };
  binary = binaries.${stdenv.hostPlatform.system};
in
# The source-built Linux compiler in the pinned nixpkgs emits undefined local
# section symbols in compiler-rt. Use the verified upstream release for Ghostty.
# Retain nixpkgs' setup hook and stdlib patch for building in Nix sandboxes.
if !stdenv.hostPlatform.isLinux then
  zig_0_16
else
  zig_0_16.overrideAttrs {
    pname = "zig-official";
    inherit version;
    src = fetchurl {
      url = "https://ziglang.org/download/${version}/zig-${binary.target}-${version}.tar.xz";
      inherit (binary) sha256;
    };
    outputs = [ "out" ];
    nativeBuildInputs = [ ];
    buildInputs = [ ];
    phases = [
      "unpackPhase"
      "patchPhase"
      "installPhase"
      "fixupPhase"
      "installCheckPhase"
    ];
    installPhase = ''
      runHook preInstall
      install -Dm755 zig "$out/bin/zig"
      mkdir -p "$out/lib"
      cp -r lib "$out/lib/zig"
      runHook postInstall
    '';
    postInstall = "";
    dontStrip = true;
    nativeInstallCheckInputs = [ binutils ];
    doInstallCheck = true;
    installCheckPhase = ''
      runHook preInstallCheck
      export ZIG_GLOBAL_CACHE_DIR="$TMPDIR/zig-check-cache"
      cat > "$TMPDIR/compiler-rt-probe.zig" <<'EOF'
      export fn probe(a: i128, b: i128) i128 { return @divTrunc(a, b); }
      EOF
      "$out/bin/zig" build-lib "$TMPDIR/compiler-rt-probe.zig" \
        -target aarch64-linux-gnu -OReleaseSafe -fPIC -fcompiler-rt \
        -femit-bin="$TMPDIR/probe.a"
      runtime=$(ar t "$TMPDIR/probe.a" | grep 'compiler_rt.o$')
      test -n "$runtime"
      ar pP "$TMPDIR/probe.a" "$runtime" > "$TMPDIR/compiler_rt.o"
      readelf -sW "$TMPDIR/compiler_rt.o" | awk '
        $4 == "SECTION" && $7 == "UND" { print; invalid = 1 }
        END { exit invalid }
      '
      runHook postInstallCheck
    '';
  }
