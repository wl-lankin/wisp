use std::path::Path;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::Result;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Os {
    Windows,
    MacOs,
    Linux,
}

impl Os {
    pub fn current() -> Self {
        if cfg!(target_os = "windows") {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::MacOs
        } else {
            Os::Linux
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fingerprint(pub [u8; 32]);

/// A hex string in JSON (the app contract and `peers.json`), raw bytes in compact binary
/// formats such as the wire protocol.
impl Serialize for Fingerprint {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            serializer.serialize_str(&self.to_hex())
        } else {
            self.0.serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for Fingerprint {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            let hex = String::deserialize(deserializer)?;
            Fingerprint::from_hex(&hex).ok_or_else(|| serde::de::Error::custom("expected 64 hex characters"))
        } else {
            <[u8; 32]>::deserialize(deserializer).map(Fingerprint)
        }
    }
}

impl Fingerprint {
    pub fn of(der: &[u8]) -> Self {
        Self(*blake3::hash(der).as_bytes())
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    pub fn from_hex(s: &str) -> Option<Self> {
        let bytes = hex::decode(s).ok()?;
        Some(Self(bytes.try_into().ok()?))
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct Identity {
    pub device_id: Uuid,
    pub name: String,
    pub os: Os,
    pub cert_der: Vec<u8>,
    pub key_der: Vec<u8>,
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identity")
            .field("device_id", &self.device_id)
            .field("name", &self.name)
            .field("os", &self.os)
            .field("cert_der", &self.cert_der)
            .field("key_der", &"<redacted>")
            .finish()
    }
}

fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(dir)
    }
}

fn write_atomic_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;

    let dir = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    create_private_dir(dir)?;
    let file_name = path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
    let tmp = dir.join(format!(".{file_name}.{}.tmp", Uuid::new_v4()));

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options.open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

impl Identity {
    pub fn generate(name: &str) -> Result<Self> {
        let device_id = Uuid::new_v4();
        let certified = rcgen::generate_simple_self_signed(vec![format!("{device_id}.wisp")])?;
        Ok(Self {
            device_id,
            name: name.to_string(),
            os: Os::current(),
            cert_der: certified.cert.der().to_vec(),
            key_der: certified.key_pair.serialize_der(),
        })
    }

    pub fn load_or_create(path: &Path, default_name: &str) -> Result<Self> {
        match std::fs::read(path) {
            Ok(bytes) => return Ok(serde_json::from_slice(&bytes)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        let identity = Self::generate(default_name)?;
        write_atomic_private(path, &serde_json::to_vec_pretty(&identity)?)?;
        Ok(identity)
    }

    pub fn fingerprint(&self) -> Fingerprint {
        Fingerprint::of(&self.cert_der)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_creates_unique_devices() {
        let a = Identity::generate("Mac").unwrap();
        let b = Identity::generate("PC").unwrap();
        assert_ne!(a.device_id, b.device_id);
        assert_ne!(a.fingerprint(), b.fingerprint());
        assert_eq!(a.name, "Mac");
    }

    #[test]
    fn load_or_create_persists_identity() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("identity.json");
        let first = Identity::load_or_create(&path, "Laptop").unwrap();
        let second = Identity::load_or_create(&path, "Other name").unwrap();
        assert_eq!(first, second);
        assert_eq!(second.name, "Laptop");
    }

    #[test]
    fn fingerprint_hex_round_trip() {
        let id = Identity::generate("X").unwrap();
        let fp = id.fingerprint();
        assert_eq!(fp.to_hex().len(), 64);
        assert_eq!(Fingerprint::from_hex(&fp.to_hex()), Some(fp));
        assert_eq!(Fingerprint::from_hex("nothex"), None);
    }

    #[test]
    fn fingerprint_is_hex_in_json_and_compact_on_the_wire() {
        let fp = Fingerprint([0xab; 32]);
        let json = serde_json::to_string(&fp).unwrap();
        assert_eq!(json, format!("\"{}\"", fp.to_hex()));
        assert_eq!(serde_json::from_str::<Fingerprint>(&json).unwrap(), fp);
        assert!(serde_json::from_str::<Fingerprint>("\"abcd\"").is_err());
        let wire = postcard::to_allocvec(&fp).unwrap();
        assert_eq!(wire.len(), 32);
        assert_eq!(postcard::from_bytes::<Fingerprint>(&wire).unwrap(), fp);
    }

    #[test]
    fn corrupt_identity_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("identity.json");
        std::fs::write(&path, b"{ not json").unwrap();
        assert!(Identity::load_or_create(&path, "X").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"{ not json");
    }

    #[test]
    fn debug_output_redacts_private_key() {
        let id = Identity::generate("X").unwrap();
        let shown = format!("{id:?}");
        assert!(shown.contains("<redacted>"));
        assert!(!shown.contains("key_der: ["));
        assert!(!shown.contains(&hex::encode(&id.key_der)));
    }

    #[test]
    fn load_or_create_leaves_no_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("identity.json");
        Identity::load_or_create(&path, "X").unwrap();
        let entries: Vec<_> = std::fs::read_dir(path.parent().unwrap()).unwrap().collect();
        assert_eq!(entries.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn identity_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("identity.json");
        Identity::load_or_create(&path, "X").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let dir_mode = std::fs::metadata(path.parent().unwrap()).unwrap().permissions().mode();
        assert_eq!(dir_mode & 0o777, 0o700);
    }
}
