use rustls::{crypto::aws_lc_rs, ClientConfig};
use rustls_platform_verifier::BuilderVerifierExt;
use std::sync::{Arc, LazyLock};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum TlsError {
    #[error("Failed to set default TLS protocol versions: {0}")]
    ProtocolVersions(#[from] rustls::Error),
}

static RUSTLS_TLS_CONFIG: LazyLock<ClientConfig> = LazyLock::new(|| {
    let provider = Arc::new(aws_lc_rs::default_provider());
    ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(TlsError::ProtocolVersions)
        .expect("TLS configuration is required for HTTPS connections")
        .with_platform_verifier()
        .with_no_client_auth()
});

pub fn tls_config() -> ClientConfig {
    RUSTLS_TLS_CONFIG.clone()
}
