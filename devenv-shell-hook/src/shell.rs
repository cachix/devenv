use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShellType {
    Bash,
    Zsh,
    Fish,
}

impl std::fmt::Display for ShellType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShellType::Bash => write!(f, "bash"),
            ShellType::Zsh => write!(f, "zsh"),
            ShellType::Fish => write!(f, "fish"),
        }
    }
}

impl std::str::FromStr for ShellType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "bash" => Ok(ShellType::Bash),
            "zsh" => Ok(ShellType::Zsh),
            "fish" => Ok(ShellType::Fish),
            _ => Err(format!("Unknown shell type: {}", s)),
        }
    }
}

pub struct ShellHook {
    shell_type: ShellType,
}

impl ShellHook {
    pub fn new(shell_type: ShellType) -> Self {
        Self { shell_type }
    }

    pub fn init_script(&self) -> String {
        match self.shell_type {
            ShellType::Bash => self.bash_init(),
            ShellType::Zsh => self.zsh_init(),
            ShellType::Fish => self.fish_init(),
        }
    }

    fn bash_init(&self) -> String {
        r#"__devenv_hook() {
    local output
    output="$(devenv shell-hook-eval --pwd "$PWD" 2>&1)"
    if [ $? -eq 0 ] && [ -n "$output" ]; then
        eval "$output"
    fi
}

if [[ ";${PROMPT_COMMAND};" != *";__devenv_hook;"* ]]; then
    PROMPT_COMMAND="__devenv_hook${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
fi"#
        .to_string()
    }

    fn zsh_init(&self) -> String {
        r#"__devenv_hook() {
    local output
    output="$(devenv shell-hook-eval --pwd "$PWD" 2>&1)"
    if [ $? -eq 0 ] && [ -n "$output" ]; then
        eval "$output"
    fi
}

if ! (( ${precmd_functions[(I)__devenv_hook]} )); then
    precmd_functions+=(__devenv_hook)
fi

if ! (( ${chpwd_functions[(I)__devenv_hook]} )); then
    chpwd_functions+=(__devenv_hook)
fi"#
        .to_string()
    }

    fn fish_init(&self) -> String {
        r#"function __devenv_hook --on-variable PWD --description 'devenv shell hook'
    set -l output (devenv shell-hook-eval --pwd "$PWD" 2>&1)
    if test $status -eq 0 -a -n "$output"
        eval "$output"
    end
end

__devenv_hook"#
            .to_string()
    }
}
