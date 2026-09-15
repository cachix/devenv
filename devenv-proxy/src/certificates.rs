use anyhow::{Context, Result, bail};
#[cfg(feature = "server")]
use openssl::pkey::Private;
use openssl::{asn1::Asn1Time, pkey::PKey, x509::X509};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TlsConfig {
    pub certificate: PathBuf,
    pub key: PathBuf,
}

impl TlsConfig {
    /// Reuse certificates while their names, CA, key, and validity still match.
    pub fn is_current(&self, hostnames: &[String], ca: &Path) -> bool {
        let check = || -> Result<bool> {
            let certificate = Certificate::load(self)?;
            let ca = X509::from_pem(&fs::read(ca)?)?;
            let ca_key = ca.public_key()?;
            let renew_at = Asn1Time::days_from_now(7)?;
            Ok(certificate.leaf().verify(&ca_key)?
                && certificate.leaf().not_after() > renew_at.as_ref()
                && hostnames
                    .iter()
                    .all(|hostname| certificate.covers(hostname)))
        };
        check().unwrap_or(false)
    }
}

pub(crate) struct Certificate {
    pub chain: Vec<X509>,
    #[cfg(feature = "server")]
    pub key: PKey<Private>,
}

impl Certificate {
    pub fn load(config: &TlsConfig) -> Result<Self> {
        let chain = X509::stack_from_pem(&fs::read(&config.certificate).with_context(|| {
            format!(
                "failed to read certificate {}",
                config.certificate.display()
            )
        })?)
        .context("failed to parse proxy certificate")?;
        let Some(leaf) = chain.first() else {
            bail!("proxy certificate file is empty")
        };
        let key =
            PKey::private_key_from_pem(&fs::read(&config.key).with_context(|| {
                format!("failed to read certificate key {}", config.key.display())
            })?)
            .context("failed to parse proxy certificate key")?;
        if !leaf.public_key()?.public_eq(&key) {
            bail!("proxy certificate and key do not match");
        }
        let now = Asn1Time::days_from_now(0)?;
        if leaf.not_before() > now.as_ref() || leaf.not_after() <= now.as_ref() {
            bail!("proxy certificate is not currently valid");
        }
        Ok(Self {
            chain,
            #[cfg(feature = "server")]
            key,
        })
    }

    pub fn leaf(&self) -> &X509 {
        &self.chain[0]
    }

    pub fn covers(&self, hostname: &str) -> bool {
        self.leaf().subject_alt_names().is_some_and(|names| {
            names.iter().any(|name| {
                name.dnsname().is_some_and(|name| {
                    name.eq_ignore_ascii_case(hostname)
                        || name.strip_prefix("*.").is_some_and(|suffix| {
                            hostname
                                .split_once('.')
                                .is_some_and(|(_, rest)| rest.eq_ignore_ascii_case(suffix))
                        })
                })
            })
        })
    }
}
