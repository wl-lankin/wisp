use rand::Rng;
use serde::{Deserialize, Serialize};
use spake2::{Ed25519Group, Identity as SpakeIdentity, Password, Spake2};
use tokio::io::{AsyncRead, AsyncWrite};
use uuid::Uuid;

use crate::identity::{Fingerprint, Identity, Os};
use crate::protocol::{read_value_max, write_value};
use crate::{CoreError, Result};

pub const MAX_PAIRING_FAILURES: u8 = 3;
pub const MAX_PAIRING_FRAME: usize = 4 * 1024;
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
///
/// Once the peer's key-exchange message has been read, the peer holds a transcript it can test
/// a code guess against offline, so every later failure (including a dropped connection) is
/// reported as `CoreError::Pairing` and callers can count it as a failed attempt.
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
    let inbound: Vec<u8> = read_value_max(stream, MAX_PAIRING_FRAME).await?;
    if inbound == outbound {
        return Err(CoreError::Pairing("peer echoed our key exchange message".into()));
    }
    exchange_info(stream, state, &outbound, &inbound, me, observed_peer)
        .await
        .map_err(|e| match e {
            CoreError::Pairing(_) => e,
            other => CoreError::Pairing(format!("pairing interrupted: {other}")),
        })
}

async fn exchange_info<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    state: Spake2<Ed25519Group>,
    outbound: &[u8],
    inbound: &[u8],
    me: &Identity,
    observed_peer: Option<Fingerprint>,
) -> Result<PairInfo> {
    let shared = state
        .finish(inbound)
        .map_err(|e| CoreError::Pairing(format!("key exchange failed: {e:?}")))?;
    let key: [u8; 32] = *blake3::hash(&shared).as_bytes();

    let mine = PairInfo {
        device_id: me.device_id,
        name: me.name.clone(),
        os: me.os,
        fingerprint: me.fingerprint(),
    };
    let mac = mac_of(&key, outbound, &mine)?;
    write_value(stream, &SignedInfo { info: mine.clone(), mac: *mac.as_bytes() }).await?;

    let theirs: SignedInfo = read_value_max(stream, MAX_PAIRING_FRAME).await?;
    // blake3::Hash equality is constant time.
    if mac_of(&key, inbound, &theirs.info)? != blake3::Hash::from(theirs.mac) {
        return Err(CoreError::Pairing("wrong code".into()));
    }
    if theirs.info.device_id == mine.device_id || theirs.info.fingerprint == mine.fingerprint {
        return Err(CoreError::Pairing("peer presented our own identity".into()));
    }
    if let Some(fp) = observed_peer {
        if fp != theirs.info.fingerprint {
            return Err(CoreError::Pairing("certificate does not match paired identity".into()));
        }
    }
    Ok(theirs.info)
}

/// The MAC covers the sender's own key-exchange message, so a MAC cannot be reflected back.
fn mac_of(key: &[u8; 32], sender_msg: &[u8], info: &PairInfo) -> Result<blake3::Hash> {
    let mut data = sender_msg.to_vec();
    data.extend_from_slice(&postcard::to_allocvec(info)?);
    Ok(blake3::keyed_hash(key, &data))
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

    #[tokio::test]
    async fn disconnect_after_key_exchange_counts_as_pairing_error() {
        let mac = Identity::generate("Mac").unwrap();
        let (mut a, mut b) = tokio::io::duplex(64 * 1024);
        let attacker = async move {
            let (_state, msg) = Spake2::<Ed25519Group>::start_symmetric(
                &Password::new(b"000000"),
                &SpakeIdentity::new(SPAKE_CONTEXT),
            );
            write_value(&mut b, &msg).await.unwrap();
            let _ours: Vec<u8> = read_value_max(&mut b, MAX_PAIRING_FRAME).await.unwrap();
            let _signed: SignedInfo = read_value_max(&mut b, MAX_PAIRING_FRAME).await.unwrap();
            drop(b);
        };
        let (r, _) = tokio::join!(pair(&mut a, "123456", &mac, None), attacker);
        assert!(matches!(r, Err(CoreError::Pairing(_))));
    }

    #[tokio::test]
    async fn reflected_frames_are_rejected() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mac = Identity::generate("Mac").unwrap();
        let (mut a, mut b) = tokio::io::duplex(64 * 1024);
        let echo = async move {
            for _ in 0..2 {
                let Ok(len) = b.read_u32().await else { return };
                let mut buf = vec![0u8; len as usize];
                if b.read_exact(&mut buf).await.is_err() {
                    return;
                }
                let _ = b.write_u32(len).await;
                let _ = b.write_all(&buf).await;
            }
        };
        let ours = async move { pair(&mut a, "123456", &mac, None).await };
        let (r, _) = tokio::join!(ours, echo);
        assert!(matches!(r, Err(CoreError::Pairing(_))));
    }
}
