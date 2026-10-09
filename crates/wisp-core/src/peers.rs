use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::identity::{Fingerprint, Os};
use crate::{CoreError, Result};

pub const MAX_ADDRESSES: usize = 8;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum DeliveryMode {
    #[default]
    Catch,
    AutoSync,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Peer {
    pub device_id: Uuid,
    pub name: String,
    pub os: Os,
    pub fingerprint: Fingerprint,
    pub addresses: Vec<SocketAddr>,
    pub mode: DeliveryMode,
}

pub struct PeerStore {
    path: PathBuf,
    peers: Vec<Peer>,
}

impl PeerStore {
    pub fn open(path: &Path) -> Result<Self> {
        let peers = if path.exists() {
            serde_json::from_slice(&std::fs::read(path)?)?
        } else {
            Vec::new()
        };
        Ok(Self { path: path.to_path_buf(), peers })
    }

    pub fn all(&self) -> &[Peer] {
        &self.peers
    }

    pub fn get(&self, id: Uuid) -> Option<&Peer> {
        self.peers.iter().find(|p| p.device_id == id)
    }

    pub fn upsert(&mut self, peer: Peer) -> Result<()> {
        match self.peers.iter_mut().find(|p| p.device_id == peer.device_id) {
            Some(existing) => {
                let mode = existing.mode;
                *existing = Peer { mode, ..peer };
            }
            None => self.peers.push(peer),
        }
        self.save()
    }

    pub fn set_mode(&mut self, id: Uuid, mode: DeliveryMode) -> Result<()> {
        self.find_mut(id)?.mode = mode;
        self.save()
    }

    pub fn remember_address(&mut self, id: Uuid, addr: SocketAddr) -> Result<()> {
        let peer = self.find_mut(id)?;
        peer.addresses.retain(|a| *a != addr);
        peer.addresses.insert(0, addr);
        peer.addresses.truncate(MAX_ADDRESSES);
        self.save()
    }

    pub fn remove(&mut self, id: Uuid) -> Result<()> {
        self.peers.retain(|p| p.device_id != id);
        self.save()
    }

    pub fn is_trusted(&self, id: Uuid, fp: &Fingerprint) -> bool {
        self.get(id).is_some_and(|p| &p.fingerprint == fp)
    }

    fn find_mut(&mut self, id: Uuid) -> Result<&mut Peer> {
        self.peers
            .iter_mut()
            .find(|p| p.device_id == id)
            .ok_or_else(|| CoreError::UnknownPeer(id.to_string()))
    }

    fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(&self.peers)?)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(name: &str) -> Peer {
        Peer {
            device_id: Uuid::new_v4(),
            name: name.into(),
            os: Os::Windows,
            fingerprint: Fingerprint([7; 32]),
            addresses: vec![],
            mode: DeliveryMode::Catch,
        }
    }

    #[test]
    fn peers_persist_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("peers.json");
        let p = peer("PC");
        {
            let mut store = PeerStore::open(&path).unwrap();
            store.upsert(p.clone()).unwrap();
        }
        let store = PeerStore::open(&path).unwrap();
        assert_eq!(store.get(p.device_id), Some(&p));
    }

    #[test]
    fn upsert_keeps_existing_mode() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = PeerStore::open(&dir.path().join("peers.json")).unwrap();
        let p = peer("PC");
        store.upsert(p.clone()).unwrap();
        store.set_mode(p.device_id, DeliveryMode::AutoSync).unwrap();
        store.upsert(Peer { name: "PC renamed".into(), ..p.clone() }).unwrap();
        let stored = store.get(p.device_id).unwrap();
        assert_eq!(stored.mode, DeliveryMode::AutoSync);
        assert_eq!(stored.name, "PC renamed");
    }

    #[test]
    fn remember_address_dedupes_orders_and_caps() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = PeerStore::open(&dir.path().join("peers.json")).unwrap();
        let p = peer("PC");
        store.upsert(p.clone()).unwrap();
        for i in 0..10u16 {
            store.remember_address(p.device_id, format!("192.168.1.{i}:4000").parse().unwrap()).unwrap();
        }
        store.remember_address(p.device_id, "192.168.1.5:4000".parse().unwrap()).unwrap();
        let addrs = &store.get(p.device_id).unwrap().addresses;
        assert_eq!(addrs.len(), MAX_ADDRESSES);
        assert_eq!(addrs[0], "192.168.1.5:4000".parse().unwrap());
        assert_eq!(addrs.iter().filter(|a| a.to_string() == "192.168.1.5:4000").count(), 1);
    }

    #[test]
    fn trust_requires_matching_fingerprint() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = PeerStore::open(&dir.path().join("peers.json")).unwrap();
        let p = peer("PC");
        store.upsert(p.clone()).unwrap();
        assert!(store.is_trusted(p.device_id, &Fingerprint([7; 32])));
        assert!(!store.is_trusted(p.device_id, &Fingerprint([8; 32])));
        assert!(!store.is_trusted(Uuid::new_v4(), &Fingerprint([7; 32])));
    }

    #[test]
    fn set_mode_on_unknown_peer_errors() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = PeerStore::open(&dir.path().join("peers.json")).unwrap();
        let err = store.set_mode(Uuid::new_v4(), DeliveryMode::AutoSync).unwrap_err();
        assert!(matches!(err, CoreError::UnknownPeer(_)));
    }
}
