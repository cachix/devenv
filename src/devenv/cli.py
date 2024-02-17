import functools
import os
import shlex
import shutil
import signal
import subprocess
import tempfile
import time
import re
import sys
import pkgutil
import json
from filelock import FileLock
from contextlib import suppress
from pathlib import Path

import click
import terminaltables
import strictyaml
import requests

from .yaml import validate_and_parse_yaml, read_yaml, write_yaml, schema
from .log import log, log_task, log_error, log_warning, log_info


NIX_FLAGS = [
    "--show-trace",
    "--extra-experimental-features",
    "nix-command",
    "--extra-experimental-features",
    "flakes",
    # remove unnecessary warnings
    "--option",
    "warn-dirty",
    "false",
    # flake caching is too aggressive
    "--option",
    "eval-cache",
    "false",
]
FILE = pkgutil.get_loader(__package__).load_module(__package__).__file__
if "site-packages" in FILE:
    SRC_DIR = Path(FILE, "..", "..", "src")
else:
    SRC_DIR = Path(FILE, "..", "..")
MODULES_DIR = (SRC_DIR / "modules").resolve()
FLAKE_FILE_TEMPL = Path(MODULES_DIR) / "flake.tmpl.nix"
FLAKE_FILE = Path(".devenv.flake.nix")
FLAKE_LOCK = "devenv.lock"

# home vars
if "XDG_DATA_HOME" not in os.environ:
    DEVENV_HOME = Path(os.environ["HOME"]) / ".devenv"
else:
    DEVENV_HOME = Path(os.environ["XDG_DATA_HOME"]) / ".devenv"
DEVENV_HOME_GC = DEVENV_HOME / "gc"
DEVENV_HOME_GC.mkdir(parents=True, exist_ok=True)
CACHIX_KNOWN_PUBKEYS = DEVENV_HOME / "cachix_pubkeys.json"

# define system like x86_64-linux
SYSTEM = (
    os.uname().machine.lower().replace("arm", "aarch")
    + "-"
    + os.uname().sysname.lower()
)


def run_nix(command: str, **kwargs) -> str:
    ctx = click.get_current_context()
    nix_flags = ctx.obj["nix_flags"]
    flags = " ".join(NIX_FLAGS) + " " + " ".join(nix_flags)
    command_flags = " ".join(ctx.obj["command_flags"])
    return run_command(f"nix {flags} {command} {command_flags}", **kwargs)


def run_command(
    command: str,
    disable_stderr=False,
    replace_shell=False,
    use_cachix=False,
    logging=True,
    dont_exit=False,
) -> str:
    nix = ""
    if command.startswith("nix"):
        if os.environ.get("DEVENV_NIX"):
            nix = os.path.join(os.environ["DEVENV_NIX"], "bin")
            command = f"{nix}/{command}"
        else:
            log(
                "$DEVENV_NIX is not set, but required as devenv doesn't work without a few Nix patches.",
                level="error",
            )
            log(
                "Please follow https://devenv.sh/getting-started/ to install devenv.",
                level="error",
            )
            exit(1)
    if use_cachix:
        caches, known_keys = get_cachix_caches(logging)
        pull_caches = " ".join(
            map(lambda cache: f"https://{cache}.cachix.org", caches.get("pull"))
        )
        command = f"{command} --option extra-trusted-public-keys '{' '.join(known_keys.values())}'"
        command = f"{command} --option extra-substituters '{pull_caches}'"
        push_cache = caches.get("push")
        if push_cache and os.environ.get("CACHIX_AUTH_TOKEN"):
            if shutil.which("cachix") is None:
                if logging:
                    log_warning(
                        "cachix is not installed, skipping pushing. Please follow https://devenv.sh/getting-started/#2-install-cachix to install cachix.",
                        level="error",
                    )
            else:
                command = f"cachix watch-exec {push_cache} {command}"

    try:
        if click.get_current_context().obj["verbose"]:
            log(f"Running command: {command}", level="debug")
        if replace_shell:
            splitted_command = shlex.split(command.strip())
            os.execv(splitted_command[0], splitted_command)
        else:
            return subprocess.run(
                command,
                shell=True,
                check=True,
                env=os.environ.copy(),
                stdout=subprocess.PIPE,
                stdin=subprocess.PIPE,
                stderr=None if not disable_stderr else subprocess.DEVNULL,
                universal_newlines=True,
            ).stdout.strip()
    except subprocess.CalledProcessError as e:
        if logging:
            click.echo("\n", err=True)
            log(
                f"Following command exited with code {e.returncode}:\n\n  {e.cmd}",
                level="error",
            )
        if dont_exit:
            raise e
        else:
            exit(e.returncode)


CONTEXT_SETTINGS = dict(max_content_width=120)


@click.group(context_settings=CONTEXT_SETTINGS)
@click.option("--verbose", "-v", help="Enable verbose output.", is_flag=True)
@click.option(
    "--nix-flags",
    "-n",
    help='Flags to pass to Nix. See `man nix.conf 5`. Example: --nix-flags "--option bash-prompt >"',
    metavar="NIX-FLAGS",
    multiple=True,
)
@click.option("--debugger", "-d", help="Enable Nix debugger.", is_flag=True)
@click.option("--system", "-s", help="Nix system to use.", default=SYSTEM)
@click.option("--offline", "-o", help="Disable network access.", is_flag=True)
@click.pass_context
def cli(ctx, offline, system, debugger, nix_flags, verbose):
    """https://devenv.sh: Fast, Declarative, Reproducible, and Composable Developer Environments."""
    ctx.ensure_object(dict)
    ctx.obj["system"] = system
    ctx.obj["verbose"] = verbose
    ctx.obj["command_flags"] = []
    ctx.obj["nix_flags"] = list(nix_flags)
    ctx.obj["nix_flags"] += ["--system", system]
    if offline:
        ctx.obj["nix_flags"] += ["--offline"]
    if debugger:
        # ignore-try is needed to avoid catching unrelated errors
        ctx.obj["command_flags"] += ["--debugger", "--ignore-try"]

    ctx.obj["gc_root"] = DEVENV_HOME_GC
    ctx.obj["gc_project"] = DEVENV_HOME_GC / str(int(time.time() * 1000))


@cli.group()
def processes():
    pass


DEVENV_DIR = Path(os.getcwd()) / ".devenv"
os.environ["DEVENV_DIR"] = str(DEVENV_DIR)
DEVENV_GC = DEVENV_DIR / "gc"
os.environ["DEVENV_GC"] = str(DEVENV_GC)

PROCESSES_PID = DEVENV_DIR / "processes.pid"
PROCESSES_LOG = DEVENV_DIR / "processes.log"


def add_gc(name, store_path):
    """Register a GC root"""
    ctx = click.get_current_context()
    run_command(
        f'nix-store --add-root "{os.environ["DEVENV_GC"]}/{name}" -r {store_path} >/dev/null'
    )
    symlink_force(store_path, f'{ctx.obj["gc_project"]}-{name}')


@cli.command(hidden=True)
@click.pass_context
def assemble(ctx):
    if not os.path.exists("devenv.nix"):
        log("File devenv.nix does not exist. To get started, run:", level="error")
        log("  $ devenv init", level="error")
        exit(1)

    DEVENV_GC.mkdir(parents=True, exist_ok=True)

    if os.path.exists("devenv.yaml"):
        validate_and_parse_yaml(DEVENV_DIR)
    else:
        for file in ["devenv.json", "flake.json", "imports.txt"]:
            file_path = DEVENV_DIR / file
            file_path.unlink(missing_ok=True)

    with open(FLAKE_FILE_TEMPL) as f:
        flake = f.read()
        system = ctx.obj["system"]

        with open(FLAKE_FILE, "w") as f:
            devenv_vars = f"""
  version = "{get_version()}";
  system = "{system}";
  devenv_root = "{os.getcwd()}";
            """
            # replace __DEVENV_VARS__ in flake using regex
            flake = re.sub(r"__DEVENV_VARS__", devenv_vars, flake)
            f.write(flake)


@cli.command(
    help="Deletes previous devenv generations. See http://devenv.sh/garbage-collection",
    short_help="Deletes previous devenv generations. See http://devenv.sh/garbage-collection",
)
@click.pass_context
def gc(ctx):
    GC_ROOTS = ctx.obj["gc_root"]
    start = time.time()

    # remove dangling symlinks
    with log_task(f"Removing non-existings symlinks in {GC_ROOTS} ..."):
        to_gc, removed_symlinks = cleanup_symlinks(GC_ROOTS)

    click.echo(f"  Found {len(to_gc)} active symlinks.")
    click.echo(f"  Deleted {len(removed_symlinks)} dangling symlinks.")
    click.echo()

    log(
        "Running garbage collection (this process may take some time) ...", level="info"
    )
    # TODO: ideally nix would report some statistics about the GC as JSON
    run_nix(f'store delete --recursive {" ".join(to_gc)}')

    after_gc, removed_symlinks = cleanup_symlinks(GC_ROOTS)
    end = time.time()

    click.echo()
    log(
        f"Done. Successfully removed {len(to_gc) - len(after_gc)} symlinks in {end - start:.0f} seconds.",
        level="info",
    )


def cleanup_symlinks(folder):
    to_gc = []
    removed_symlinks = []
    for root, dirs, files in os.walk(folder):
        for name in files:
            full_path = os.path.join(root, name)
            if os.path.islink(full_path):
                if not os.path.isfile(full_path):
                    os.unlink(full_path)
                    removed_symlinks.append(full_path)
                else:
                    to_gc.append(full_path)
    return to_gc, removed_symlinks


def get_dev_environment(ctx, json=False, logging=True):
    ctx.invoke(assemble)
    if logging:
        action = log_task("Building shell")
    else:
        action = suppress()
    with action:
        gc_root = DEVENV_GC / "shell"
        cmd = f"print-dev-env --profile '{gc_root}'"
        if json:
            cmd += " --json"
        env = run_nix(cmd, logging=False, use_cachix=True)
        run_command(
            f"nix-env -p '{gc_root}' --delete-generations old",
            logging=False,
            disable_stderr=True,
        )
        symlink_force(gc_root, Path(f'{ctx.obj["gc_project"]}-shell'))
    return env, gc_root


@cli.command(
    help="Activate the developer environment.",
    short_help="Activate the developer environment.",
    context_settings=dict(
        ignore_unknown_options=True,
    ),
)
@click.argument("cmd", required=False)
@click.argument("extra_args", nargs=-1, type=click.UNPROCESSED)
@click.pass_context
def shell(ctx, cmd, extra_args):
    env, gc_root = get_dev_environment(ctx)
    if cmd:
        run_nix(
            f"develop '{gc_root}' -c {cmd} {' '.join(extra_args)}", replace_shell=True
        )
    else:
        log("Entering shell", level="info")
        run_nix(f"develop '{gc_root}'", replace_shell=True)


def symlink_force(src, dst):
    src = Path(src)
    dst = Path(dst)
    # locking is needed until https://github.com/python/cpython/pull/14464
    with FileLock(f"{dst}.lock", timeout=10):
        dst.unlink(missing_ok=True)
        dst.symlink_to(src)


@cli.command(
    help="Starts processes in foreground. See http://devenv.sh/processes",
    short_help="Starts processes in foreground. See http://devenv.sh/processes",
)
@click.argument("process", required=False)
@click.option(
    "--detach", "-d", is_flag=True, help="Starts processes in the background."
)
@click.pass_context
def up(ctx, process, detach):
    with log_task("Building processes"):
        ctx.invoke(assemble)
        procfilescript = run_nix(
            "build --no-link --print-out-paths '.#procfileScript'", use_cachix=True
        )
    with open(procfilescript, "r") as file:
        contents = file.read().strip()
    if contents == "":
        log(
            "No 'processes' option defined: https://devenv.sh/processes/", level="error"
        )
        exit(1)
    else:
        env, _ = get_dev_environment(ctx)
        log("Starting processes ...", level="info")
        add_gc("procfilescript", procfilescript)
        processes_script = os.path.join(DEVENV_DIR, "processes")
        with open(processes_script, "w") as f:
            f.write(
                f"""#!/usr/bin/env bash
{env}
exec {procfilescript} {process or ""}
            """
            )
        os.chmod(processes_script, 0o755)

        if detach:
            process = subprocess.Popen(
                [processes_script],
                stdout=open(PROCESSES_LOG, "w"),
                stderr=subprocess.STDOUT,
            )

            with open(PROCESSES_PID, "w") as file:
                file.write(str(process.pid))
            log(f"  PID is {process.pid}.", level="info")
            log(f"  See logs:  $ tail -f {PROCESSES_LOG}", level="info")
            log("  Stop:      $ devenv processes stop", level="info")
        else:
            os.execv(processes_script, [processes_script])


processes.add_command(up)


@processes.command(
    help="Stops processes started with 'devenv up'.",
    short_help="Stops processes started with 'devenv up'.",
)
def stop():
    with log_task("Stopping processes", newline=False):
        if not os.path.exists(PROCESSES_PID):
            log("No processes running.", level="error")
            exit(1)

        with open(PROCESSES_PID, "r") as file:
            pid = int(file.read())

        log(f"Stopping process with PID {pid} ...", level="info")

        try:
            os.kill(pid, signal.SIGTERM)
        except ProcessLookupError:
            log(f"Process with PID {pid} not found.", level="error")
            exit(1)

        os.remove(PROCESSES_PID)


@cli.command()
@click.argument("name")
@click.pass_context
def search(ctx, name):
    """Search packages matching NAME in nixpkgs input."""
    ctx.invoke(assemble)
    options = run_nix(
        "build --no-link --print-out-paths '.#optionsJSON' ", use_cachix=True
    )
    search = run_nix(f"search --json nixpkgs {name}")

    with open(Path(options) / "share" / "doc" / "nixos" / "options.json") as f:
        options_results = []
        for key, value in json.load(f).items():
            if name in key:
                options_results.append(
                    (
                        key,
                        value["type"],
                        value.get("default", ""),
                        value["description"][:80],
                    )
                )
        results_options_count = len(options_results)

    search_results = []
    for key, value in json.loads(search).items():
        search_results.append(
            (
                "pkgs." + (".".join(key.split(".")[2:])),
                value["version"],
                value["description"][:80],
            )
        )
    search_results_count = len(search_results)

    if search_results:
        click.echo(
            terminaltables.AsciiTable(
                [("Package", "Version", "Description")] + search_results
            ).table
        )

    if options_results:
        click.echo(
            terminaltables.AsciiTable(
                [("Option", "Type", "Default", "Description")] + options_results
            ).table
        )

    log(
        f"Found {search_results_count} packages and {results_options_count} options for '{name}'.",
        level="info",
    )


@cli.command(
    help="Build, copy and run a container. See http://devenv.sh/containers",
    short_help="Build, copy and run a container. See http://devenv.sh/containers",
)
@click.option(
    "--registry",
    default=None,
    help="Registry to copy the container to.",
    metavar="REGISTRY",
)
@click.option("--copy", is_flag=True, help="Copy the container to the registry.")
@click.option(
    "--copy-args",
    default=None,
    help="Arguments passed to `skopeo copy`.",
    metavar="ARGS",
)
@click.option("--docker-run", is_flag=True, help="Execute `docker run`.")
@click.argument("container_name")
@click.pass_context
def container(ctx, registry, copy, copy_args, docker_run, container_name):
    os.environ["DEVENV_CONTAINER"] = container_name

    with log_task(f"Building {container_name} container"):
        ctx.invoke(assemble)
        # NOTE: we need --impure here to read DEVENV_CONTAINER
        spec = run_nix(
            f'build --impure --print-out-paths --no-link .#devenv.containers."{container_name}".derivation',
            use_cachix=True,
        )
        click.echo(spec)

    # copy container
    if copy or docker_run:
        with log_task(f"Copying {container_name} container"):
            # we need --impure here for DEVENV_CONTAINER
            copy_script = run_nix(
                f'build --print-out-paths --no-link \
            --impure .#devenv.containers."{container_name}".copyScript',
                use_cachix=True,
            )

            if docker_run:
                registry = "docker-daemon:"

            cp = f"{copy_script} {spec} {registry or 'false'} {copy_args or ''}"

            log(f"Running '{cp}'", level="info")

            subprocess.run(
                cp,
                shell=True,
                check=True,
            )

    if docker_run:
        log(f"Starting {container_name} container", level="info")
        # we need --impure here for DEVENV_CONTAINER
        docker_script = run_nix(
            f'build --print-out-paths --no-link --impure \
              .#devenv.containers."{container_name}".dockerRun',
            use_cachix=True,
        )

        subprocess.run(docker_script)


@cli.command(
    help="Print information about this developer environment.",
    short_help="Print information about this developer environment.",
)
@click.pass_context
def info(ctx):
    ctx.invoke(assemble)
    # TODO: use --json and reconstruct input metadata
    metadata = run_nix("flake metadata")
    matches = re.search(r"(Inputs:.+)$", metadata, re.DOTALL)
    if matches:
        inputs = matches.group(1)
    else:
        inputs = ""
    info_ = run_nix("eval --raw '.#info'")
    click.echo(f"{inputs}\n{info_}")


@cli.command()
@click.pass_context
def version(ctx):
    """Display devenv version."""
    version = get_version()
    click.echo(f"devenv {version} ({ctx.obj['system']})")


@cli.command(
    help="Scaffold devenv.yaml, devenv.nix, and .envrc.",
    short_help="Scaffold devenv.yaml, devenv.nix, and .envrc.",
)
@click.argument("target", default=".")
def init(target):
    os.makedirs(target, exist_ok=True)

    required_files = ["devenv.nix", "devenv.yaml", ".envrc"]
    for filename in required_files:
        if os.path.exists(Path(target, filename)):
            log(f"Aborting since {filename} already exist.", level="error")
            exit(1)
            return

    example = "simple"
    examples_path = Path(MODULES_DIR / ".." / ".." / "examples").resolve()

    for filename in required_files:
        full_filename = Path(target, filename)
        if not os.path.exists(full_filename):
            log(f"Creating {full_filename}", level="info")
            shutil.copyfile(
                os.path.join(examples_path, example, filename), full_filename
            )

    with open(".gitignore", "a+") as gitignore_file:
        if "devenv" not in gitignore_file.read():
            log("Appending defaults to .gitignore", level="info")
            gitignore_file.write("\n")
            gitignore_file.write("# Devenv\n")
            gitignore_file.write(".devenv*\n")
            gitignore_file.write("devenv.local.nix\n")
            gitignore_file.write("\n")
            gitignore_file.write("# direnv\n")
            gitignore_file.write(".direnv\n")
            gitignore_file.write("\n")
            gitignore_file.write("# pre-commit\n")
            gitignore_file.write(".pre-commit-config.yaml\n")
            gitignore_file.write("\n")

    log("Done.", level="info")

    # Check if direnv is installed
    if shutil.which("direnv"):
        log("direnv is installed. Running $ direnv allow .", level="info")
        subprocess.run(["direnv", "allow"])


@cli.command(
    help="Update devenv.lock from devenv.yaml inputs. See http://devenv.sh/inputs/",
    short_help="Update devenv.lock from devenv.yaml inputs. See http://devenv.sh/inputs/",
)
@click.argument("input_name", required=False)
@click.pass_context
def update(ctx, input_name):
    ctx.invoke(assemble)

    if input_name:
        run_nix(f"flake lock --update-input {input_name}")
    else:
        run_nix("flake update")


@cli.command()
@click.pass_context
def ci(ctx):
    """Builds your developer environment and checks if everything builds."""
    ctx.invoke(assemble)
    print("running ci")
    print(run_command("cat ${FLAKE_FILE}"))
    output_path = run_nix("build --no-link --print-out-paths .#ci", use_cachix=True)
    add_gc("ci", output_path)


@cli.command(hidden=True)
@click.option("--json", is_flag=True)
@click.pass_context
def print_dev_env(ctx, json):
    env, _ = get_dev_environment(ctx, json=json, logging=False)
    click.echo(env)


def get_version():
    with open(Path(MODULES_DIR, "latest-version")) as f:
        return f.read().strip()


@cli.group(
    help="Manage inputs in devenv.yaml. See http://devenv.sh/inputs/",
    short_help="Manage inputs in devenv.yaml. See http://devenv.sh/inputs/",
)
def inputs():
    pass


@inputs.command(
    help="Add a new input to the developer environment.",
    short_help="Add a new input to the developer environment.",
)
@click.argument("name")
@click.argument("url")
@click.option("--follows", "-f", multiple=True, help="Add a dependency to the input.")
@click.pass_context
def add(ctx, name, url, follows):
    devenv = read_yaml()
    attrs = {"url": url}

    inputs = {}
    for follow in follows:
        if follow not in devenv["inputs"]:
            log(
                f"Input {follow} does not exist so it can't be followed.", level="error"
            )
            exit(1)
        inputs[follow] = {"follows": follow}

    if inputs:
        attrs["inputs"] = inputs
    devenv["inputs"][name] = attrs

    write_yaml(devenv)


@cli.command(
    help="Run tests. See http://devenv.sh/tests/",
    short_help="Run tests. See http://devenv.sh/tests/",
)
@click.argument("names", nargs=-1)
@click.option("--debug", is_flag=True, help="Run tests in debug mode.")
@click.option("--keep-going", is_flag=True, help="Continue running tests if one fails.")
@click.option(
    "--exclude",
    multiple=True,
    help="A test name to exclude, may be specified multiple times",
)
@click.pass_context
def test(ctx, debug, keep_going, exclude, names):
    ctx.invoke(assemble)
    with log_task("Gathering tests", newline=False):
        tests = json.loads(run_nix("eval .#devenv.tests --json"))

    if not names:
        names = ["local"]

    # group tests by tags
    tags = {}
    for name, test in tests.items():
        for tag in test["tags"]:
            if tag not in tags:
                tags[tag] = []
            tags[tag].append(name)

    selected_tests = []
    for name in names:
        if name in tests:
            selected_tests.append(name)
        tag_tests = tags.get(name, {})
        for test in tag_tests:
            if not test in exclude:
                selected_tests.append(test)

    log(f"Found {len(tests)} test(s), running {len(selected_tests)}:", level="info")

    pwd = os.getcwd()
    failed = []

    for name in selected_tests:
        with log_task(f"  Testing {name}"):
            with tempfile.TemporaryDirectory(prefix=name + "_") as tmpdir:
                os.chdir(tmpdir)
                test = tests[name]

                if test.get("src"):
                    shutil.copytree(
                        test["src"], ".", dirs_exist_ok=True, copy_function=shutil.copy
                    )
                    run_command("find . -type d -exec chmod +wx {} \;")
                else:
                    write_if_defined("devenv.nix", test.get("nix"))
                    write_if_defined("devenv.yaml", test.get("yaml"))
                    write_if_defined(".test.sh", test.get("test"))
                    if os.path.exists(".test.sh"):
                        os.chmod(".test.sh", 0o755)

                # predefined utilities
                write_if_defined(
                    "devenv.local.nix",
                    """
{ pkgs, ... }: {
  packages = [ pkgs.coreutils-full ];
}
                """.strip()
                    + "\n",
                )

                # plug in devenv input if needed
                if os.path.exists(os.path.join(pwd, "src/modules/latest-version")):
                    log(
                        "    Detected devenv module. Using src/modules for tests.",
                        level="info",
                    )

                    modules = os.path.join(pwd, "src/modules")
                    if not os.path.exists("devenv.yaml"):
                        write_yaml(
                            strictyaml.as_document({"inputs": {}}, schema=schema)
                        )
                    os.chmod("devenv.yaml", 0o644)
                    yaml = read_yaml()
                    inputs = yaml.get("inputs", {})
                    inputs["devenv"] = {"url": f"path:{modules}"}
                    yaml["inputs"] = inputs
                    write_yaml(yaml)

                devenv = sys.argv[0]
                has_processes = False
                try:
                    log("    Running $ devenv ci ...", level="info")
                    run_command(f"{devenv} ci")

                    has_processes = os.path.exists(
                        ".devenv/gc/ci"
                    ) and "-devenv-up" in run_command("cat .devenv/gc/ci")

                    if has_processes:
                        log("    Starting processes ...", level="info")
                        run_command(f"{devenv} up -d")
                        # stream logs
                        p = subprocess.Popen(
                            "tail -f .devenv/processes.log",
                            shell=True,
                        )
                    else:
                        p = None

                    try:
                        if os.path.exists(".test.sh"):
                            log("    Running .test.sh ...", level="info")
                            run_command(f"{devenv} shell bash ./.test.sh")
                    finally:
                        if has_processes and not debug:
                            run_command(f"{devenv} processes stop")
                            if p:
                                p.kill()
                except KeyboardInterrupt:
                    raise
                except BaseException as e:
                    log_error(f"Test {name} failed.")
                    if keep_going:
                        failed.append(name)
                        continue
                    if debug:
                        log(
                            "Entering shell because of the --debug flag:",
                            level="warning",
                        )
                        log(f"  - devenv: {devenv}", level="warning")
                        log(f"  - cwd: {tmpdir}", level="warning")
                        if has_processes:
                            log("  - up logs: .devenv/processes.log:", level="warning")
                        os.execv("/bin/sh", ["/bin/sh"])
                    else:
                        log_warning("Pass --debug flag to enter shell.")
                        raise e
    if keep_going and failed:
        log_error(f"Failed: {', '.join(failed)}")
        sys.exit(2)


def write_if_defined(file, content):
    if content:
        with open(file, "w") as f:
            f.write(content)


@functools.cache
def get_cachix_caches(logging=True):
    """Get the full list of cachix caches we need and their public keys.

    This is cached because it's expensive to run.
    """
    try:
        caches_raw = run_nix(
            "eval .#devenv.cachix --json",
            dont_exit=True,
            disable_stderr=True,
            logging=False,
        )
    except subprocess.CalledProcessError:
        return {"pull": [], "push": None}, {}

    caches = json.loads(caches_raw)

    if CACHIX_KNOWN_PUBKEYS.exists():
        known_keys = json.loads(CACHIX_KNOWN_PUBKEYS.read_text())
    else:
        known_keys = {}
    new_known_keys = {}
    for name in caches.get("pull", []):
        if name not in known_keys:
            resp = requests.get(f"https://cachix.org/api/v1/cache/{name}")
            if resp.status_code in [401, 404]:
                log_error(
                    f"Cache {name} does not exist or you don't have a CACHIX_AUTH_TOKEN configured."
                )
                # TODO: instruct how to best configure netrc
                # log_error("To configure a token, run `cachix authtoken <token>`.")
                log_error("To create a cache, go to https://app.cachix.org/.")
                exit(1)
            else:
                resp.raise_for_status()
                pubkey = resp.json()["publicSigningKeys"][0]
                new_known_keys[name] = pubkey

    if caches.get("pull"):
        if logging:
            log_info(f"Using Cachix: {', '.join(caches.get('pull', []))} ")
        if new_known_keys:
            for name, pubkey in new_known_keys.items():
                if logging:
                    log_info(
                        f"  Trusting {name}.cachix.org on first use with the public key {pubkey}"
                    )
            known_keys.update(new_known_keys)
        CACHIX_KNOWN_PUBKEYS.write_text(json.dumps(known_keys))
    return caches, known_keys


@cli.command()
@click.argument("attrs", nargs=-1, required=True)
@click.pass_context
def build(ctx, attrs):
    """Build attributes in your devenv.nix."""
    ctx.invoke(assemble)
    attrs = " ".join(map(lambda attr: f".#devenv.{attr}", attrs))
    output = run_nix(
        f"build --print-out-paths --print-build-logs --no-link {attrs}", use_cachix=True
    )
    log("Built:", level="info")
    for path in output.splitlines():
        log(path, level="info")
