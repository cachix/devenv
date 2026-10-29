//! Select the consuming process and hand SecretSpec's file lease to it.
use miette::{IntoDiagnostic, Result};
use std::{
    io,
    sync::{Arc, Mutex},
};

#[derive(Clone)]
pub(crate) struct SecretFiles {
    owner: Option<u32>,
    lease: Arc<Mutex<Option<secretspec::FileLease>>>,
}
impl SecretFiles {
    pub(crate) fn new(owner: Option<u32>) -> Self {
        Self {
            owner,
            lease: Arc::new(Mutex::new(None)),
        }
    }
    pub(crate) fn configure(&self, secrets: &mut secretspec::Secrets) {
        let files = self.clone();
        secrets.set_as_path_materializer(move |bytes, suffix| {
            let mut lease = files
                .lease
                .lock()
                .map_err(|_| io::Error::other("Secret file lease lock poisoned"))?;
            if lease.is_none() {
                let owner = match files.owner {
                    Some(owner) => owner,
                    None => export_owner_pid()?,
                };
                *lease = Some(secretspec::FileLease::new(owner));
            }
            lease.as_ref().unwrap().materialize(bytes, suffix)
        });
    }
    pub(crate) fn handoff(&self) -> Result<()> {
        self.handoff_to(None)
    }
    pub(crate) fn retain_for_current_process(&self) -> Result<()> {
        if self.owner == Some(std::process::id()) {
            self.handoff()?;
        }
        Ok(())
    }
    pub(crate) fn handoff_to(&self, owner: Option<u32>) -> Result<()> {
        if let Some(lease) = self
            .lease
            .lock()
            .map_err(|_| miette::miette!("Secret file lease lock poisoned"))?
            .as_ref()
        {
            match owner {
                Some(owner) => lease.handoff_to(owner),
                None => lease.handoff(),
            }
            .into_diagnostic()?;
        }
        Ok(())
    }
}

fn export_owner_pid() -> io::Result<u32> {
    let mut owner =
        secretspec::file_lease::ProcessInfo::read(nix::unistd::getppid().as_raw() as u32)?;
    if std::env::var_os("DIRENV_IN_ENVRC").is_some() {
        let mut found = false;
        for _ in 0..128 {
            if owner
                .executable
                .file_name()
                .is_some_and(|name| name == "direnv")
            {
                match owner.arguments.get(1).map(|arg| arg.as_os_str()) {
                    Some(command) if command == "export" => {
                        owner = secretspec::file_lease::ProcessInfo::read(owner.parent)?
                    }
                    // direnv replaces itself with the requested command.
                    Some(command) if command == "exec" => return Ok(owner.pid),
                    _ => return Err(io::Error::other("Unsupported direnv secret consumer")),
                }
                found = true;
                break;
            }
            if owner.parent <= 1 {
                return Err(io::Error::other("Cannot find the consuming direnv shell"));
            }
            owner = secretspec::file_lease::ProcessInfo::read(owner.parent)?;
        }
        if !found {
            return Err(io::Error::other("Cannot find the consuming direnv shell"));
        }
    }
    for _ in 0..128 {
        if owner.executed {
            break;
        }
        let parent = secretspec::file_lease::ProcessInfo::read(owner.parent)?;
        if owner.group != parent.group || owner.executable != parent.executable {
            break;
        }
        owner = parent;
    }
    Ok(owner.pid)
}
