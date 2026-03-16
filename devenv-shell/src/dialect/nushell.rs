use super::{InteractiveArgs, RcfileContext, ShellDialect};
use std::path::{Path, PathBuf};

/// Nushell dialect implementation.
///
/// Architecture: We always launch bash first to source the devenv environment
/// (which produces bash syntax). The bash rcfile computes the env diff, saves
/// `_DEVENV_PATH`, then execs into nushell with `--config` and `--env-config`
/// pointing to our custom init files. Nushell inherits environment variables
/// from the parent bash process, so the devenv environment carries over.
///
/// For hot-reload, nushell calls a bash helper script (written by
/// `write_init_files`) to compute env diffs, then parses `export -p` output
/// to apply changes back into the nushell environment.
pub struct NushellDialect;

impl NushellDialect {
    /// Return the nushell config directory.
    ///
    /// Checks `XDG_CONFIG_HOME` first, then falls back to `~/.config`.
    fn nu_config_dir() -> Option<PathBuf> {
        let config_home = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
        config_home.map(|p| p.join("nushell"))
    }

    /// Return the path to the user's nushell config.nu, if it exists.
    fn user_config_nu() -> Option<PathBuf> {
        Self::nu_config_dir()
            .map(|d| d.join("config.nu"))
            .filter(|p| p.exists())
    }

    /// Return the path to the user's nushell env.nu, if it exists.
    fn user_env_nu() -> Option<PathBuf> {
        Self::nu_config_dir()
            .map(|d| d.join("env.nu"))
            .filter(|p| p.exists())
    }
}

impl ShellDialect for NushellDialect {
    fn name(&self) -> &str {
        "nu"
    }

    fn interactive_args(&self) -> InteractiveArgs {
        // We always launch bash first, then exec into nushell from the rcfile.
        super::BashDialect.interactive_args()
    }

    fn rcfile_content(&self, ctx: &RcfileContext) -> String {
        let target_shell = ctx.target_shell_path.unwrap_or("nu");
        let nu_dir = ctx.init_dir.join("nu");
        let nu_config = nu_dir.join("config.nu");
        let nu_env = nu_dir.join("env.nu");

        format!(
            r#"# Disable history during init so devenv internal commands do not pollute history.
set +o history

# Environment diff helpers (always defined for tracking)
{env_diff_helpers}

# Capture environment BEFORE sourcing devenv (for diff tracking)
_devenv_before_file=$(mktemp)
__devenv_capture_env > "$_devenv_before_file"

# Source the devenv environment
source "{env_script_path}"

# Compute and store the initial diff in _DEVENV_DIFF env var
__devenv_compute_diff "$_devenv_before_file"
rm -f "$_devenv_before_file"
unset _devenv_before_file

# Save PATH before nushell init potentially modifies it
export _DEVENV_PATH="$PATH"

# Re-enable history before exec
set -o history

# Exec into nushell with our custom config files.
# Nushell inherits env vars from the parent process, so the devenv
# environment carries over automatically.
if [ ! -x "{target_shell}" ] && ! command -v "{target_shell}" >/dev/null 2>&1; then
    echo "devenv: error: shell '{target_shell}' not found" >&2
    echo "devenv: add nushell to your devenv.nix packages or set SHELL to an absolute path" >&2
fi
exec "{target_shell}" --config "{nu_config}" --env-config "{nu_env}"
echo "devenv: error: failed to exec into {target_shell}" >&2
"#,
            env_diff_helpers = ctx.env_diff_helpers,
            env_script_path = ctx.env_script_path.to_string_lossy(),
            target_shell = target_shell,
            nu_config = nu_config.to_string_lossy(),
            nu_env = nu_env.to_string_lossy(),
        )
    }

    fn env_diff_helpers(&self) -> &str {
        // Reuse the same bash helpers; they are sourced in bash before exec to nushell
        super::BashDialect.env_diff_helpers()
    }

    fn reload_hook(&self, reload_file: &Path) -> String {
        // For nushell, the reload hook is generated directly in write_init_files
        // because it needs to reference the bash helper script path (init_dir).
        // We return the reload_file path as a marker so write_init_files knows
        // reload is enabled and which file to watch.
        reload_file.to_string_lossy().to_string()
    }

    fn user_rcfile(&self) -> Option<PathBuf> {
        Self::nu_config_dir().map(|d| d.join("config.nu"))
    }

    fn prompt_prefix(&self) -> &str {
        // Nushell prompt is set via $env.PROMPT_COMMAND; the actual prefix
        // is applied in the generated config.nu via write_init_files.
        ""
    }

    fn write_init_files(&self, ctx: &RcfileContext) -> std::io::Result<()> {
        let nu_dir = ctx.init_dir.join("nu");
        std::fs::create_dir_all(&nu_dir)?;

        // Build the source line for the user's env.nu if it exists.
        // Nushell requires source paths to be known at parse time, so we
        // embed the resolved path as a literal.
        let user_env_source = match Self::user_env_nu() {
            Some(path) => format!("source \"{}\"", path.to_string_lossy()),
            None => "# No user env.nu found".to_string(),
        };

        let env_nu_content = format!(
            r#"# devenv nushell env init
# Source user's env.nu for their environment customizations
{user_env_source}
"#,
            user_env_source = user_env_source,
        );

        // Build the source line for the user's config.nu if it exists.
        let user_config_source = match Self::user_config_nu() {
            Some(path) => format!("source \"{}\"", path.to_string_lossy()),
            None => "# No user config.nu found".to_string(),
        };

        // Generate reload hook code. ctx.reload_hook contains the reload file
        // path when reload is enabled, or an empty string when disabled.
        let reload_code = if ctx.reload_hook.is_empty() {
            String::new()
        } else {
            let reload_file = ctx.reload_hook;
            let helper_script = nu_dir.join("reload_helper.sh");

            // Write the bash helper script that handles the env diff.
            // This avoids quoting issues from embedding bash code in nushell strings.
            let bash_helper = format!(
                r#"#!/usr/bin/env bash
{env_diff_helpers}

# Reverse previous diff
__devenv_apply_reverse_diff

# Capture env before sourcing new devenv
_before=$(mktemp)
__devenv_capture_env > "$_before"

# Source new devenv environment
source "{reload_file}"
rm -f "{reload_file}"

# Compute new diff
__devenv_compute_diff "$_before"
rm -f "$_before"

# Output current environment for nushell to parse
export -p
"#,
                env_diff_helpers = super::BashDialect.env_diff_helpers(),
                reload_file = reload_file,
            );
            std::fs::write(&helper_script, &bash_helper)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&helper_script, std::fs::Permissions::from_mode(0o755))?;
            }

            let helper_path_str = helper_script.to_string_lossy();

            format!(
                r#"
# --- devenv hot-reload support ---

# Apply a reload: run the bash helper script to compute the env diff,
# then parse `export -p` output and apply environment changes.
def __devenv_reload_apply [] {{
    let reload_file = "{reload_file}"
    if ($reload_file | path exists) {{
        let bash_output = (bash "{helper_path}" | complete)
        if ($bash_output.exit_code == 0) {{
            # Parse `declare -x VAR="value"` lines from bash export -p output
            for line in ($bash_output.stdout | lines) {{
                let trimmed = ($line | str trim)
                if ($trimmed | str starts-with "declare -x ") {{
                    let vardef = ($trimmed | str substring 11..)
                    let eq_pos = ($vardef | str index-of "=")
                    if $eq_pos >= 0 {{
                        let var_name = ($vardef | str substring ..$eq_pos)
                        let raw_value = ($vardef | str substring ($eq_pos + 1)..)
                        # Strip surrounding double quotes if present
                        let value = if ($raw_value | str starts-with '"') {{
                            $raw_value | str trim --char '"'
                        }} else {{
                            $raw_value
                        }}
                        # Skip shell internal variables
                        if $var_name not-in ["BASH" "BASHOPTS" "BASH_ARGC" "BASH_ARGV" "BASH_LINENO" "BASH_SOURCE" "BASH_VERSINFO" "BASH_VERSION" "SHELLOPTS" "SHLVL" "OLDPWD" "_"] {{
                            # PATH is a list in nushell; split colon-separated values
                            if $var_name == "PATH" {{
                                $env.PATH = ($value | split row ":")
                            }} else {{
                                load-env {{($var_name): $value}}
                            }}
                        }}
                    }}
                }}
            }}
            # Update saved PATH as colon-separated string
            $env._DEVENV_PATH = ($env.PATH | str join ":")
        }}
    }}
}}

# Keybinding for manual reload (Ctrl+Alt+R)
$env.config.keybindings = ($env.config.keybindings? | default [] | append {{
    name: devenv_reload
    modifier: control_alt
    keycode: char_r
    mode: [emacs vi_normal vi_insert]
    event: {{ send: executehostcommand cmd: "__devenv_reload_apply" }}
}})

# Pre-prompt hook: restore devenv PATH and check for pending reloads.
# _DEVENV_PATH is a colon-separated string; split into a list for nushell.
$env.config.hooks.pre_prompt = ($env.config.hooks.pre_prompt? | default [] | append {{||
    if ("_DEVENV_PATH" in $env) {{
        $env.PATH = ($env._DEVENV_PATH | split row ":")
    }}
}})
"#,
                reload_file = reload_file,
                helper_path = helper_path_str,
            )
        };

        let config_nu_content = format!(
            r#"# devenv nushell config init

# Source user's config.nu for their customizations (aliases, keybindings, etc.)
{user_config_source}

# Restore devenv PATH after user config may have modified it.
# _DEVENV_PATH is a colon-separated string from bash; split into a list.
$env.PATH = ($env._DEVENV_PATH | split row ":")

# Prepend (devenv) to the prompt.
# Save the original prompt command before overriding, then wrap it.
let _devenv_original_prompt = (if "PROMPT_COMMAND" in $env {{ $env.PROMPT_COMMAND }} else {{ {{|| ""}} }})
$env.PROMPT_COMMAND = {{|| "(devenv) " + (do $_devenv_original_prompt) }}

# Hot-reload hook
{reload_code}
"#,
            user_config_source = user_config_source,
            reload_code = reload_code,
        );

        std::fs::write(nu_dir.join("env.nu"), env_nu_content)?;
        std::fs::write(nu_dir.join("config.nu"), config_nu_content)?;
        Ok(())
    }
}
