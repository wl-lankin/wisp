use std::path::Path;

use crate::Result;

/// Creates the directory (and parents); on unix it is restricted to the owner, also when
/// it already existed with looser permissions.
pub fn create_private_dir(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Writes the file; on unix it is readable by the owner only, also when it already existed.
pub fn write_private_file(path: &Path, bytes: &[u8]) -> Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        file.write_all(bytes)?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, bytes)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_nested_private_dir() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a").join("b");
        create_private_dir(&nested).unwrap();
        assert!(nested.is_dir());
        create_private_dir(&nested).unwrap();
    }

    #[test]
    fn writes_and_overwrites_private_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("f.bin");
        write_private_file(&file, b"long contents").unwrap();
        write_private_file(&file, b"short").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"short");
    }
}
