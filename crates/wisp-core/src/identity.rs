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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Fingerprint(pub [u8; 32]);

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Identity {
    pub device_id: Uuid,
    pub name: String,
    pub os: Os,
    pub cert_der: Vec<u8>,
    pub key_der: Vec<u8>,
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
        if path.exists() {
            return Ok(serde_json::from_slice(&std::fs::read(path)?)?);
        }
        let identity = Self::generate(default_name)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_vec_pretty(&identity)?)?;
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
}
