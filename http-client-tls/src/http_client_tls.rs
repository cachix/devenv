use rustls::{ClientConfig, crypto::aws_lc_rs};
use rustls_platform_verifier::BuilderVerifierExt;
use std::sync::{Arc, LazyLock};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum TlsError {
    #[error("Failed to set default TLS protocol versions: {0}")]
    ProtocolVersions(rustls::Error),

    #[error("Failed to initialize platform verifier: {0}")]
    PlatformVerifier(rustls::Error),
}

static RUSTLS_TLS_CONFIG: LazyLock<ClientConfig> = LazyLock::new(|| {
    let provider = Arc::new(aws_lc_rs::default_provider());
    ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(TlsError::ProtocolVersions)
        .unwrap()
        .with_platform_verifier()
        .map_err(TlsError::PlatformVerifier)
        .unwrap()
        .with_no_client_auth()
});

pub fn tls_config() -> ClientConfig {
    RUSTLS_TLS_CONFIG.clone()
}
