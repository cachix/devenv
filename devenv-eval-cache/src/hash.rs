use miette::{IntoDiagnostic, Result};
use std::path::Path;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::AsyncWrite;
use tokio::{fs::File, io};

pub(crate) fn digest(input: &str) -> String {
    let hash = blake3::hash(input.as_bytes());
    hash.to_hex().as_str().to_string()
}

pub(crate) async fn compute_file_hash<P: AsRef<Path>>(path: P) -> io::Result<String> {
    let mut file = File::open(path).await?;
    let mut hasher = Blake3Async::new();
    io::copy(&mut file, &mut hasher).await?;
    Ok(hasher.inner.finalize().to_hex().as_str().to_string())
}

/// A newtype around `blake3::Hasher` that implements `AsyncWrite`.
/// This makes it compatible with `tokio::io::copy`.
#[derive(Default)]
struct Blake3Async {
    inner: blake3::Hasher,
}

impl Blake3Async {
    fn new() -> Self {
        Self {
            inner: blake3::Hasher::new(),
        }
    }
}

impl AsyncWrite for Blake3Async {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.get_mut().inner.update(buf);
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}
