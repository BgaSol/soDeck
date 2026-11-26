use std::{io::Read, path::Path};

pub(crate) const LIMIT: usize = 1024 * 1024;

pub(crate) fn read(path: &Path) -> Result<String, ()> {
    let mut text = String::new();
    std::fs::File::open(path)
        .map_err(|_| ())?
        .take(LIMIT as u64 + 1)
        .read_to_string(&mut text)
        .map_err(|_| ())?;
    if text.len() > LIMIT {
        return Err(());
    }
    Ok(text)
}

pub(crate) fn inspect(text: &str) -> Result<bool, ()> {
    if text.len() > LIMIT {
        return Err(());
    }
    let key = russh::keys::PrivateKey::from_openssh(text).map_err(|_| ())?;
    if !matches!(
        key.algorithm(),
        russh::keys::Algorithm::Ed25519 | russh::keys::Algorithm::Rsa { .. }
    ) {
        return Err(());
    }
    Ok(key.is_encrypted())
}
