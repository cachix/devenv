use std::path::Path;
use std::{fs, io};

pub(crate) fn digest<T: AsRef<str>>(input: T) -> String {
    let hash = blake3::hash(input.as_ref().as_bytes());
    hash.to_hex().as_str().to_string()
}

pub(crate) fn compute_file_hash<P: AsRef<Path>>(path: P) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    io::copy(&mut file, &mut hasher)?;
    Ok(hasher.finalize().to_hex().as_str().to_string())
}
