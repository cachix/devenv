"""Executable checks for environment composition helpers.

Run with Python 3 and --nixpkgs /path/to/nixpkgs. Nix needs access to its daemon.
Artifacts and build outputs are kept in a temporary directory.
"""

import argparse
import json
import pathlib
import subprocess
import tempfile


SOURCES = pathlib.Path(__file__).resolve().parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--nixpkgs", type=pathlib.Path, required=True)
parser.add_argument("--library", type=pathlib.Path, default=SOURCES / "../../src/modules/lib/env.nix")
parser.add_argument("--compat-nixpkgs", type=pathlib.Path)
args = parser.parse_args()
artifacts = tempfile.TemporaryDirectory(prefix="devenv-env-helpers-")
ROOT = pathlib.Path(artifacts.name)
NIX = ["nix", "--extra-experimental-features", "nix-command"]
LIBRARY_ARGS = ["--argstr", "envLibrary", str(args.library.resolve())]
PACKAGE_ARGS = ["--argstr", "nixpkgs", str(args.nixpkgs.resolve())]


def checked(command, *, output=None, **kwargs):
    result = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, **kwargs)
    if result.returncode:
        raise AssertionError(
            f"Command failed: {command!r}\n{result.stderr.decode(errors='replace')[-5000:]}"
        )
    if output:
        (ROOT / output).write_bytes(result.stdout)
    return result.stdout


def capture(scenario):
    data = checked(
        NIX + ["print-dev-env", "--file", str(SOURCES / "probe.nix"),
               "--argstr", "scenario", scenario, "--json", *LIBRARY_ARGS, *PACKAGE_ARGS],
        output=f"{scenario}.json",
    )
    return json.loads(data)["variables"]


def value(variables, name):
    entry = variables[name]
    assert entry["type"] == "exported", (name, entry)
    return entry["value"]


package_sets = [args.nixpkgs]
if args.compat_nixpkgs:
    package_sets.append(args.compat_nixpkgs)
for package_set in package_sets:
    checks = json.loads(checked([
        "nix-instantiate", "--eval", "--strict", "--json",
        str(SOURCES / "eval.nix"), "-A", "checks", *LIBRARY_ARGS,
        "--argstr", "nixpkgs", str(package_set.resolve()),
    ]))
    assert checks and all(checks.values()), checks
    print(f"PASS: {len(checks)} evaluation checks with {package_set}", flush=True)

normal = capture("normal")
expected = {
    "CC": "zig cc",
    "CONFIG_SHELL": "/custom/bin/sh",
    "SEQUENCE": "earliest:first:second:hook:initial:last:latest",
    "BOTH": "after",
    "SETUP_SAW_BOTH": "before",
    "EMPTY": "",
    "EXISTING_EMPTY": "",
    "UNSET_DEFAULT": "fallback",
    "ZERO_SEPARATOR": "leftmiddleright",
    "REPLACE_AND_EXTEND": "prefix:base:suffix",
    "DEFAULT_AND_EXTEND": "prefix:fallback:suffix",
    "PREPEND_UNSET": "prefix",
    "APPEND_UNSET": "suffix",
    "QUOTED": "literal ' \" $HOME $(false) `false`\nsecond line\n",
    "ODD_SEPARATOR": 'left$(false)"middle',
    "TYPED_LIST": "one two three",
    "TYPED_BOOL": "1",
    "TYPED_FALSE": "",
    "LEGACY_LIST": "one two",
    "LEGACY_BOOL": "1",
}
for name, wanted in expected.items():
    assert value(normal, name) == wanted, (name, value(normal, name), wanted)
assert "LEGACY_NULL" not in normal
assert value(normal, "TEST_OVERRIDE") == "retained"
assert value(normal, "TYPED_PATH").startswith("/nix/store/")
assert pathlib.Path(value(normal, "TYPED_PATH")).read_bytes() == (SOURCES / "probe.nix").read_bytes()
assert value(normal, "TYPED_PACKAGE").startswith("/nix/store/")
assert value(normal, "PKG_CONFIG_PATH").startswith("/custom/lib/pkgconfig:")
assert value(normal, "NIX_CFLAGS_COMPILE").endswith(" -DCUSTOM")
assert "-isystem /nix/store/" in value(normal, "NIX_CFLAGS_COMPILE")
assert value(normal, "NIX_LDFLAGS").endswith(" -L/custom/lib")
assert not any(name.startswith("_devenv_env_") and name != "_devenv_env_final_values" for name in normal)
assert "devenvEnvFinalPath" not in normal
assert not any(name.startswith("_devenv_env_") for name in json.loads((ROOT / "normal.json").read_text())["bashFunctions"])
print("PASS: setup timing, final precedence, ordering, fallback, coercion, quoting and cleanup", flush=True)

environment = {name: entry["value"] for name, entry in normal.items() if entry["type"] == "exported"}
# Raw captures include temporary paths belonging to Nix's now-finished sandbox.
# Keep the captured toolchain and flags, but give subprocesses live temp paths.
environment.update({name: str(ROOT) for name in ("TMPDIR", "TMP", "TEMP", "TEMPDIR")})
checked(["pkg-config", "--exists", "zlib"], env=environment)
source = b"#ifndef CUSTOM\n#error missing custom flag\n#endif\n#include <zlib.h>\nint main(void) { return zlibVersion() == 0; }\n"
checked([value(normal, "PROBE_COMPILER"), "-x", "c", "-", "-o", str(ROOT / "zlib-probe"), "-lz"], input=source, env=environment)
checked([str(ROOT / "zlib-probe")], env=environment)
print("PASS: pkg-config discovery, custom compiler flag, zlib compilation, linking and execution", flush=True)

activation = checked(
    NIX + ["print-dev-env", "--file", str(SOURCES / "probe.nix"), *LIBRARY_ARGS, *PACKAGE_ARGS],
    output="activation.sh",
)
names = list(expected) + ["PKG_CONFIG_PATH", "TYPED_PATH", "TYPED_PACKAGE"]
actual = checked([
    "bash", "--noprofile", "--norc", "-c",
    'source "$1"; shift; for name; do printf "%s\\0" "${!name}"; done',
    "bash", str(ROOT / "activation.sh"), *names,
])
assert actual.split(b"\0") == [value(normal, name).encode() for name in names] + [b""]
print("PASS: sourceable Bash and JSON agree, including trailing newlines", flush=True)

large = capture("large")
assert value(large, "LARGE_ONE") == "a" * 70000
assert value(large, "LARGE_TWO") == "b" * 70000
print("PASS: two 70 KB values avoid the single-environment-string size limit", flush=True)

structured = capture("structured")
for name, wanted in expected.items():
    if name not in {"LEGACY_LIST", "LEGACY_BOOL"}:
        assert value(structured, name) == wanted, (name, structured.get(name), wanted)
print("PASS: explicit operations and initial values work with structured derivation attributes", flush=True)

overrides = capture("overrides")
for name, wanted in expected.items():
    assert value(overrides, name) == wanted, (name, overrides.get(name), wanted)
assert value(overrides, "TEST_MKSHELL_OVERRIDE") == "retained"
assert value(overrides, "TEST_OVERRIDE") == "chained"
print("PASS: chained override, overrideAttrs and overrideDerivation preserve final operations", flush=True)

path = capture("path")
assert value(path, "PATH") == "/configured/bin"
path_activation = checked(
    NIX + ["print-dev-env", "--file", str(SOURCES / "probe.nix"),
           "--argstr", "scenario", "path", *LIBRARY_ARGS, *PACKAGE_ARGS],
    output="path-activation.sh",
)
actual_path = checked([
    "bash", "--noprofile", "--norc", "-c",
    'source "$1"; printf "%s" "$PATH"', "bash", str(ROOT / "path-activation.sh"),
])
assert actual_path == b"/configured/bin"
build = json.loads(checked(
    NIX + ["build", "--no-link", "--json", "--file", str(SOURCES / "probe.nix"),
           "--argstr", "scenario", "path", *LIBRARY_ARGS, *PACKAGE_ARGS],
))
output = build[0]["outputs"]["out"]
references = checked(["nix-store", "--query", "--references", output]).decode().splitlines()
assert value(path, "FINAL_ONLY_PACKAGE") in references, references
stdenv = pathlib.Path(value(normal, "stdenv"))
assert (stdenv / "nix-support").is_dir()
print("PASS: final PATH survives activation, shell builds succeed, and helper-only dependencies are retained", flush=True)
