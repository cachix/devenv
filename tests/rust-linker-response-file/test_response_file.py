"""Exercise rustc's linker response file beyond Linux's exec argument limit."""

import os
from pathlib import Path
import resource
import subprocess
import sys
import tempfile


def compile_rust(arguments):
    # Let the Nix compiler wrappers pass response files through, so GCC reaches
    # collect2 instead of hitting the same exec limit in the wrapper itself.
    return subprocess.run(
        ["rustc", *arguments],
        env=os.environ | {"NIX_CC_USE_RESPONSE_FILE": "1"},
        capture_output=True,
        text=True,
        timeout=120,
    )


def diagnostic(stderr):
    # rustc includes the enormous linker command in its error. Keep the useful
    # diagnostic lines without dumping megabytes of search paths into CI logs.
    return "\n".join(line for line in stderr.splitlines() if len(line) < 2000)[-8000:]


def main():
    gcc, clang = sys.argv[1:]
    # Linux derives ARG_MAX from the stack limit. Bound it in this test process
    # so unlimited-stack shells do not turn the probe into a huge, slow link.
    _, hard_stack = resource.getrlimit(resource.RLIMIT_STACK)
    stack = 4 * 1024 * 1024
    if hard_stack != resource.RLIM_INFINITY:
        stack = min(stack, hard_stack)
    resource.setrlimit(resource.RLIMIT_STACK, (stack, hard_stack))
    with tempfile.TemporaryDirectory(prefix="rust-linker-response-") as directory:
        workdir = Path(directory)
        source = workdir / "main.rs"
        source.write_text('fn main() { println!("response file linked"); }\n')

        # First rule out unrelated compiler or toolchain failures.
        small = compile_rust(
            [str(source), "-C", f"linker={gcc}", "-o", str(workdir / "small")]
        )
        if small.returncode:
            raise AssertionError(f"small GCC link failed:\n{diagnostic(small.stderr)}")

        # Give rustc its own response file so the compiler invocation stays small.
        # Distinct native search paths survive rustc's deduplication and force it
        # to use a second response file when spawning the linker driver.
        limit = os.sysconf("SC_ARG_MAX")
        paths = []
        size = 0
        while size <= limit * 5 // 4:
            argument = f"-Lnative={workdir}/{'unused-' * 24}/{len(paths):08d}"
            paths.append(argument)
            size += len(argument.encode()) + 1

        response = workdir / "rustc.rsp"

        def large_link(driver, name):
            output = workdir / name
            response.write_text(
                "\n".join(
                    [str(source), "-C", f"linker={driver}", "-o", str(output), *paths]
                )
                + "\n"
            )
            return compile_rust([f"@{response}"]), output

        # GCC expands the response file before trying to execute collect2.
        broken, _ = large_link(gcc, "gcc-large")
        if (
            broken.returncode == 0
            or "Argument list too long" not in broken.stderr
            or "collect2" not in broken.stderr
        ):
            raise AssertionError(
                "did not reproduce GCC's collect2 argument limit:\n"
                + diagnostic(broken.stderr)
            )

        # Use exactly the driver selected by the Rust module, including its LLD
        # wrapper on x86_64 Linux. Clang must pass the large link to the linker.
        fixed, executable = large_link(clang, "clang-large")
        if fixed.returncode:
            raise AssertionError(
                f"large Clang link failed:\n{diagnostic(fixed.stderr)}"
            )
        result = subprocess.run(
            [str(executable)], capture_output=True, text=True, check=True, timeout=10
        )
        if result.stdout != "response file linked\n":
            raise AssertionError(f"unexpected executable output: {result.stdout!r}")
        print(
            f"GCC reproduced E2BIG; configured Clang linked {size} bytes successfully"
        )


if __name__ == "__main__":
    main()
