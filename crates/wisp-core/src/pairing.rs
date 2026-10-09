use rand::Rng;
use serde::{Deserialize, Serialize};
use spake2::{Ed25519Group, Identity as SpakeIdentity, Password, Spake2};
use tokio::io::{AsyncRead, AsyncWrite};
use uuid::Uuid;

use crate::identity::{Fingerprint, Identity, Os};
use crate::protocol::{read_value, write_value};
use crate::{CoreError, Result};

pub const MAX_PAIRING_FAILURES: u8 = 3;
const SPAKE_CONTEXT: &[u8] = b"wisp-pair-v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PairInfo {
    pub device_id: Uuid,
    pub name: String,
    pub os: Os,
    pub fingerprint: Fingerprint,
}

#[derive(Serialize, Deserialize)]
struct SignedInfo {
    info: PairInfo,
    mac: [u8; 32],
}

/// Both sides send before they verify, so neither blocks waiting for the other.
pub async fn pair<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    code: &str,
    me: &Identity,
    observed_peer: Option<Fingerprint>,
) -> Result<PairInfo> {
    let (state, outbound) = Spake2::<Ed25519Group>::start_symmetric(
        &Password::new(code.as_bytes()),
        &SpakeIdentity::new(SPAKE_CONTEXT),
    );
    write_value(stream, &outbound).await?;
    let inbound: Vec<u8> = read_value(stream).await?;
    let shared = state
        .finish(&inbound)
        .map_err(|e| CoreError::Pairing(format!("key exchange failed: {e:?}")))?;
    let key: [u8; 32] = *blake3::hash(&shared).as_bytes();

    let mine = PairInfo {
        device_id: me.device_id,
        name: me.name.clone(),
        os: me.os,
        fingerprint: me.fingerprint(),
    };
    let mac = mac_of(&key, &mine)?;
    write_value(stream, &SignedInfo { info: mine, mac: *mac.as_bytes() }).await?;

    let theirs: SignedInfo = read_value(stream).await?;
    // blake3::Hash equality is constant time.
    if mac_of(&key, &theirs.info)? != blake3::Hash::from(theirs.mac) {
        return Err(CoreError::Pairing("wrong code".into()));
    }
    if let Some(fp) = observed_peer {
        if fp != theirs.info.fingerprint {
            return Err(CoreError::Pairing("certificate does not match paired identity".into()));
        }
    }
    Ok(theirs.info)
}

fn mac_of(key: &[u8; 32], info: &PairInfo) -> Result<blake3::Hash> {
    let bytes = postcard::to_allocvec(info)?;
    Ok(blake3::keyed_hash(key, &bytes))
}

pub struct PairingSession {
    code: String,
    failures: u8,
}

impl PairingSession {
    pub fn new() -> Self {
        let n: u32 = rand::rngs::OsRng.gen_range(0..1_000_000);
        Self { code: format!("{n:06}"), failures: 0 }
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn is_open(&self) -> bool {
        self.failures < MAX_PAIRING_FAILURES
    }

    pub fn record_failure(&mut self) {
        self.failures = self.failures.saturating_add(1);
    }
}

impl Default for PairingSession {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn same_code_exchanges_identities() {
        let mac = Identity::generate("Mac").unwrap();
        let pc = Identity::generate("PC").unwrap();
        let (mut a, mut b) = tokio::io::duplex(64 * 1024);
        let (ra, rb) = tokio::join!(
            pair(&mut a, "123456", &mac, Some(pc.fingerprint())),
            pair(&mut b, "123456", &pc, Some(mac.fingerprint())),
        );
        let seen_by_mac = ra.unwrap();
        let seen_by_pc = rb.unwrap();
        assert_eq!(seen_by_mac.device_id, pc.device_id);
        assert_eq!(seen_by_mac.fingerprint, pc.fingerprint());
        assert_eq!(seen_by_pc.device_id, mac.device_id);
        assert_eq!(seen_by_pc.name, "Mac");
    }

    #[tokio::test]
    async fn wrong_code_fails_on_both_sides() {
        let mac = Identity::generate("Mac").unwrap();
        let pc = Identity::generate("PC").unwrap();
        let (mut a, mut b) = tokio::io::duplex(64 * 1024);
        let (ra, rb) = tokio::join!(
            pair(&mut a, "123456", &mac, None),
            pair(&mut b, "654321", &pc, None),
        );
        assert!(matches!(ra, Err(CoreError::Pairing(_))));
        assert!(matches!(rb, Err(CoreError::Pairing(_))));
    }

    #[tokio::test]
    async fn fingerprint_not_matching_tls_certificate_is_rejected() {
        let mac = Identity::generate("Mac").unwrap();
        let pc = Identity::generate("PC").unwrap();
        let attacker = Identity::generate("Evil").unwrap();
        let (mut a, mut b) = tokio::io::duplex(64 * 1024);
        let (ra, _rb) = tokio::join!(
            pair(&mut a, "123456", &mac, Some(attacker.fingerprint())),
            pair(&mut b, "123456", &pc, None),
        );
        assert!(matches!(ra, Err(CoreError::Pairing(_))));
    }

    #[test]
    fn session_closes_after_three_failures() {
        let mut s = PairingSession::new();
        assert_eq!(s.code().len(), 6);
        assert!(s.code().chars().all(|c| c.is_ascii_digit()));
        s.record_failure();
        s.record_failure();
        assert!(s.is_open());
        s.record_failure();
        assert!(!s.is_open());
    }
}
