import os
import shutil
import subprocess
import time
import re
from filelock import FileLock
from contextlib import suppress
from pathlib import Path
import pkgutil
import json

import click
import terminaltables


from .yaml import validate_and_parse_yaml, read_yaml, write_yaml
from .log import log, log_task


NIX_FLAGS = [
    "--show-trace",
    "--extra-experimental-features",
    "\"nix-command flakes\"",
    "--option",
    "warn-dirty",
    "false",
]
FILE = pkgutil.get_loader(__package__).load_module(__package__).__file__
if 'site-packages' in FILE:
    SRC_DIR = Path(FILE, '..', '..', 'src')
else:
    SRC_DIR = Path(FILE, '..', '..')
MODULES_DIR = (SRC_DIR / 'modules').resolve()
FLAKE_FILE_TEMPL = os.path.join(MODULES_DIR, "flake.tmpl.nix")
FLAKE_FILE = ".devenv.flake.nix"
FLAKE_LOCK = "devenv.lock"

# define system like x86_64-linux
SYSTEM = os.uname().machine.lower().replace("arm", "aarch") + "-" + os.uname().sysname.lower()

def run_nix(command: str) -> str:
    ctx = click.get_current_context()
    nix_flags = ctx.obj['nix_flags']
    flags = " ".join(NIX_FLAGS) + " " + " ".join(nix_flags)
    command_flags = " ".join(ctx.obj['command_flags'])
    return run_command(f"nix {flags} {command} {command_flags}")

def run_command(command: str) -> str:
    if command.startswith("nix"):
        if os.environ.get("DEVENV_NIX"):
            nix = os.path.join(os.environ["DEVENV_NIX"], "bin")
            command = f"{nix}/{command}"
        else:
            log("$DEVENV_NIX is not set, but required as devenv doesn't work without a few Nix patches.", level="error")
            log("Please follow https://devenv.sh/getting-started/ to install devenv.", level="error")
            exit(1)
    try:
        return subprocess.run(
            command, 
            shell=True,
            check=True, 
            env=os.environ.copy(),
            stdout=subprocess.PIPE,
            universal_newlines=True).stdout.strip()
    except subprocess.CalledProcessError as e:
        if e.returncode == 130:
            pass  # we're exiting the shell
        else:
            click.echo("\n", err=True)
            log(f"Following command exited with code {e.returncode}:\n\n  {e.cmd}", level="error")
            exit(e.returncode)

CONTEXT_SETTINGS = dict(max_content_width=120)

@click.group(context_settings=CONTEXT_SETTINGS)
@click.option(
    '--nix-flags', '-n', 
    help='Flags to pass to Nix. See `man nix.conf 5`. Example: --nix-flags "--option bash-prompt >"',
    metavar="NIX-FLAGS",
    multiple=True)
@click.option(
    '--debugger', '-d',
    help='Enable Nix debugger.',
    is_flag=True)
@click.option(
    '--system', '-s',
    help='Nix system to use.',
    default=SYSTEM)
@click.option(
    '--offline', '-o',
    help='Disable network access.',
    is_flag=True)
@click.option(
    '--disable-eval-cache',
    help='Disable Nix evaluation cache.',
    is_flag=True)
@click.pass_context
def cli(ctx, disable_eval_cache, offline, system, debugger, nix_flags):
    """https://devenv.sh: Fast, Declarative, Reproducible, and Composable Developer Environments."""
    ctx.ensure_object(dict)
    ctx.obj['system'] = system
    ctx.obj['command_flags'] = []
    ctx.obj['nix_flags'] = list(nix_flags)
    ctx.obj['nix_flags'] += ['--system', system]
    if offline:
        ctx.obj['nix_flags'] += ['--offline']
    if debugger:
        # ignore-try is needed to avoid catching unrelated errors
        ctx.obj['command_flags'] += ['--debugger', '--ignore-try']
        # to avoid confusing errors
        disable_eval_cache = True
    if disable_eval_cache:
        ctx.obj['nix_flags'] += ['--option', 'eval-cache', 'false']

    if 'XDG_DATA_HOME' not in os.environ:
        ctx.obj['gc_root'] = os.path.join(os.environ['HOME'], '.devenv', 'gc')
    else:
        ctx.obj['gc_root'] = os.path.join(os.environ['XDG_DATA_HOME'], 'devenv', 'gc')
    
    Path(ctx.obj['gc_root']).mkdir(parents=True, exist_ok=True)
    ctx.obj['gc_project'] = os.path.join(ctx.obj['gc_root'], str(int(time.time() * 1000)))


def add_gc(name, store_path):
    """Register a GC root"""
    ctx = click.get_current_context()
    run_command(f'nix-store --add-root "{os.environ["DEVENV_GC"]}/{name}" -r {store_path} >/dev/null')
    os.symlink(store_path, f'{ctx.obj["gc_project"]}-{name}', True)


@cli.command(hidden=True)
@click.pass_context
def assemble(ctx):
    if not os.path.exists('devenv.nix'):
        log('File devenv.nix does not exist. To get started, run:', level="error")
        log('  $ devenv init', level="error")
        exit(1)

    DEVENV_DIR = Path(os.getcwd()) / '.devenv'
    os.environ['DEVENV_DIR'] = str(DEVENV_DIR)
    DEVENV_GC = DEVENV_DIR / 'gc'
    os.environ['DEVENV_GC'] = str(DEVENV_GC)
    DEVENV_GC.mkdir(parents=True, exist_ok=True)

    if os.path.exists('devenv.yaml'):
        validate_and_parse_yaml(DEVENV_DIR)
    else:
        for file in ['devenv.json', 'flake.json', 'imports.txt']:
            file_path = DEVENV_DIR / file
            if file_path.exists():
                os.remove(file_path)

    with open(FLAKE_FILE_TEMPL) as f:
        flake = f.read()
        system = ctx.obj['system']

        with open(FLAKE_FILE, 'w') as f:
            devenv_vars = (f"""
  version = "{get_version()}";
  system = "{system}";
  devenv_root = "{os.getcwd()}";
            """)
            # replace __DEVENV_VARS__ in flake using regex
            flake = re.sub(r'__DEVENV_VARS__', devenv_vars, flake)
            f.write(flake)


@cli.command(
    help="Deletes previous devenv generations. See http://devenv.sh/garbage-collection",
    short_help="Deletes previous devenv generations. See http://devenv.sh/garbage-collection",
)
@click.pass_context
def gc(ctx):
    GC_ROOTS = ctx.obj['gc_root']
    start = time.time()

    # remove dangling symlinks
    with log_task(f'Removing non-existings symlinks in {GC_ROOTS} ...'):
        to_gc, removed_symlinks = cleanup_symlinks(GC_ROOTS)

    click.echo(f'  Found {len(to_gc)} active symlinks.')
    click.echo(f'  Deleted {len(removed_symlinks)} dangling symlinks.')
    click.echo()

    log(f'Running garbage collection (this process may take some time) ...', level="info")
    # TODO: ideally nix would report some statistics about the GC as JSON
    run_nix(f'store delete --recursive {" ".join(to_gc)}')

    after_gc, removed_symlinks = cleanup_symlinks(GC_ROOTS)
    end = time.time()

    click.echo()
    log(f'Done. Successfully removed {len(to_gc) - len(after_gc)} symlinks in {end - start:.0f} seconds.', level="info")

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

def get_dev_environment(ctx, is_shell=False):
    ctx.invoke(assemble)
    if is_shell:
        action = log_task('Building shell')
    else:
        action = suppress()
    with action:
        gc_root = os.path.join(os.environ['DEVENV_GC'], 'shell')
        env = run_nix(f"print-dev-env --impure --profile '{gc_root}'")
        run_command(f"nix-env -p '{gc_root}' --delete-generations old")
        symlink_force(Path(f'{ctx.obj["gc_project"]}-shell'), gc_root)
    return env, gc_root



@cli.command(
    help="Activate the developer environment.",
    short_help="Activate the developer environment.",
    context_settings=dict(
        ignore_unknown_options=True,
    )
)
@click.argument('extra_args', nargs=-1, type=click.UNPROCESSED)
@click.argument('cmd', required=False)
@click.pass_context
def shell(ctx, cmd, extra_args):
    env, gc_root = get_dev_environment(ctx, is_shell=True)
    if cmd:
        run_nix(f"develop '{gc_root}' -c {cmd} {' '.join(extra_args)}")
    else:
        log('Entering shell', level="info")
        run_nix(f"develop '{gc_root}'")
        
def symlink_force(src, dst):
    # locking is needed until https://github.com/python/cpython/pull/14464
    with FileLock(f"{dst}.lock", timeout=10):
        src.unlink(missing_ok=True)
        Path(src).symlink_to(dst)

@cli.command(
    help="Starts processes in foreground. See http://devenv.sh/processes", 
    short_help="Starts processes in foreground. See http://devenv.sh/processes",
)
@click.argument('command', required=False)
@click.pass_context
def up(ctx, command):
    with log_task('Building processes'):
        ctx.invoke(assemble)
        procfilescript = run_nix(f"build --no-link --print-out-paths --impure '.#procfileScript'")
    with open(procfilescript, 'r') as file:
        contents = file.read().strip()
    if contents == '':
        log("No 'processes' option defined: https://devenv.sh/processes/", level="error")
        exit(1)
    else:
        log('Starting processes ...', level="info")
        add_gc('procfilescript', procfilescript)
        # TODO: print output to stdout
        #run_command(procfilescript + ' ' + (command or ''))
        args = [] if not command else [command]
        subprocess.run([procfilescript] + args)

@cli.command()
@click.argument('name')
@click.pass_context
def search(ctx, name):
    """Search packages matching NAME in nixpkgs input."""
    ctx.invoke(assemble)
    options = run_nix(f"build --no-link --print-out-paths '.#optionsJSON' --impure")
    search = run_nix(f"search --json nixpkgs {name}")

    with open(Path(options) / 'share' / 'doc' / 'nixos' / 'options.json') as f:
        options_results = []
        for key, value in json.load(f).items():
            if name in key:
                options_results.append((
                    key,
                    value['type'],
                    value['default'],
                    value['description'][:80]
                ))
        results_options_count = len(options_results)

    search_results = []
    for key, value in json.loads(search).items():
        search_results.append(
            (".".join(key.split('.')[2:])
            , value['version']
            , value['description'][:80]
            )
        )
    search_results_count = len(search_results)

    if search_results:
        click.echo(
            terminaltables.AsciiTable(
                [("Package", "Version", "Description")] 
                + search_results
            ).table
        )

    if options_results:
        click.echo(
            terminaltables.AsciiTable(
                [("Option", "Type", "Default", "Description")] 
                + options_results
            ).table
        )
    
    log(f"Found {search_results_count} packages and {results_options_count} options for '{name}'.", level="info")

@cli.command(
    help="Build, copy and run a container. See http://devenv.sh/containers",
    short_help="Build, copy and run a container. See http://devenv.sh/containers",
)
@click.option('--registry', default=None, help='Registry to copy the container to.', metavar="REGISTRY")
@click.option('--copy', is_flag=True, help='Copy the container to the registry.')
@click.option('--copy-args', default=None, help='Arguments passed to `skopeo copy`.', metavar="ARGS")
@click.option('--docker-run', is_flag=True, help='Execute `docker run`.')
@click.argument('container_name')
@click.pass_context
def container(ctx, registry, copy, copy_args, docker_run, container_name):
    os.environ['DEVENV_CONTAINER'] = container_name

    with log_task(f'Building {container_name} container'):
        ctx.invoke(assemble)
        # NOTE: we need --impure here to read DEVENV_CONTAINER
        spec = run_nix(f"build --impure --print-out-paths --no-link .#devenv.containers.\"{container_name}\".derivation")
        click.echo(spec)
  
    # copy container
    if copy or docker_run:
        with log_task(f'Copying {container_name} container'):
            copy_script = run_nix(f"build --print-out-paths --no-link \
            --impure .#devenv.containers.\"{container_name}\".copyScript")
            
            if docker_run:
                registry = "docker-daemon:"
            
            subprocess.run(
                f"{copy_script} {spec} {registry} {copy_args or ''}",
                shell=True,
                check=True)

    if docker_run:
        with log_task(f'Starting {container_name} container'):
            docker_script = run_nix(f"build --print-out-paths --no-link --impure \
              .#devenv.containers.\"{container_name}\".dockerRun")
            
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
    info_ = run_nix("eval --raw '.#info' --impure")
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
@click.argument('target', default='.')
def init(target):
    os.makedirs(target, exist_ok=True)

    required_files = ['devenv.nix', 'devenv.yaml', '.envrc']
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
            shutil.copyfile(os.path.join(examples_path, example, filename), full_filename)

    with open('.gitignore', 'a+') as gitignore_file:
        if 'devenv' not in gitignore_file.read():
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
    if shutil.which('direnv'):
        log("direnv is installed. Running $ direnv allow .", level="info")
        subprocess.run(['direnv', 'allow'])

@cli.command(
    help="Update devenv.lock from devenv.yaml inputs. See http://devenv.sh/inputs/",
    short_help="Update devenv.lock from devenv.yaml inputs. See http://devenv.sh/inputs/",
)
@click.argument('input_name', required=False)
@click.pass_context
def update(ctx, input_name):
    ctx.invoke(assemble)

    if input_name:
        run_nix(f"flake lock --update-input {input_name}")
    else:
        run_nix(f"flake update")

@cli.command()
@click.pass_context
def ci(ctx):
    """Builds your developer environment and checks if everything builds."""
    ctx.invoke(assemble)
    output_path = run_nix(f"build --no-link --print-out-paths --impure .#ci")
    add_gc('ci', output_path)

@cli.command()
@click.pass_context
def print_dev_env(ctx):
    env, _ = get_dev_environment(ctx)
    click.echo(env)

def get_version():
    with open(Path(MODULES_DIR, "latest-version")) as f:
        return f.read().strip()

@cli.group(
    help="Manage inputs in devenv.yaml. See http://devenv.sh/inputs/",
    short_help="Manage inputs in devenv.yaml. See http://devenv.sh/inputs/"     
)
def inputs():
    pass

@inputs.command(
    help="Add a new input to the developer environment.",
    short_help="Add a new input to the developer environment.",
)
@click.argument('name')
@click.argument('url')
@click.option('--follows', '-f', multiple=True, help='Add a dependency to the input.')
@click.pass_context
def add(ctx, name, url, follows):
    devenv = read_yaml()
    attrs = {'url': url}

    inputs = {}
    for follow in follows:
        if follow not in devenv['inputs']:
            log(f"Input {follow} does not exist so it can't be followed.", level="error")
            exit(1)
        inputs[follow] = {"follows": follow}

    if inputs:
        attrs['inputs'] = inputs
    devenv['inputs'][name] = attrs
    
    write_yaml(devenv)