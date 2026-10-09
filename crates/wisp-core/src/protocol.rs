use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use uuid::Uuid;

use crate::identity::Os;
use crate::item::{ClipPayload, ItemKind, Preview};
use crate::{CoreError, Result};

pub const MAX_FRAME: usize = 64 * 1024 * 1024;
pub const PROTOCOL_VERSION: u16 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Message {
    Hello { device_id: Uuid, name: String, os: Os, version: u16 },
    /// `live` is false for offers replayed from the offline queue; receivers never auto-pull those.
    Offer { item_id: Uuid, kind: ItemKind, size: u64, preview: Preview, live: bool },
    /// Only ever sent in reply to `Pull`.
    Push { item_id: Uuid, payload: ClipPayload },
    Pull { item_id: Uuid },
    FileStart { transfer_id: Uuid, name: String, size: u64, hash: [u8; 32] },
    FileChunk { transfer_id: Uuid, offset: u64, bytes: Vec<u8> },
    FileAck { transfer_id: Uuid, offset: u64 },
    FileDone { transfer_id: Uuid },
    Ping { nonce: u64 },
    Pong { nonce: u64 },
}

pub async fn write_value<W: AsyncWrite + Unpin, T: Serialize>(w: &mut W, value: &T) -> Result<()> {
    let bytes = postcard::to_allocvec(value)?;
    if bytes.len() > MAX_FRAME {
        return Err(CoreError::Frame(format!("frame of {} bytes exceeds limit", bytes.len())));
    }
    w.write_u32(bytes.len() as u32).await?;
    w.write_all(&bytes).await?;
    w.flush().await?;
    Ok(())
}

pub async fn read_value<R: AsyncRead + Unpin, T: DeserializeOwned>(r: &mut R) -> Result<T> {
    read_value_max(r, MAX_FRAME).await
}

pub async fn read_value_max<R: AsyncRead + Unpin, T: DeserializeOwned>(r: &mut R, max: usize) -> Result<T> {
    let len = r.read_u32().await? as usize;
    if len > max {
        return Err(CoreError::Frame(format!("frame of {len} bytes exceeds limit")));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).await?;
    Ok(postcard::from_bytes(&buf)?)
}

pub async fn write_frame<W: AsyncWrite + Unpin>(w: &mut W, msg: &Message) -> Result<()> {
    write_value(w, msg).await
}

pub async fn read_frame<R: AsyncRead + Unpin>(r: &mut R) -> Result<Message> {
    read_value(r).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{ClipPayload, ItemKind, Preview};
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn frames_round_trip() {
        let (mut a, mut b) = tokio::io::duplex(1024 * 1024);
        let msgs = vec![
            Message::Hello { device_id: Uuid::new_v4(), name: "Mac".into(), os: Os::MacOs, version: PROTOCOL_VERSION },
            Message::Offer { item_id: Uuid::new_v4(), kind: ItemKind::Text, size: 5, preview: Preview::Text("hello".into()), live: true },
            Message::Push { item_id: Uuid::new_v4(), payload: ClipPayload::Text("hello".into()) },
            Message::Ping { nonce: 7 },
        ];
        for m in &msgs {
            write_frame(&mut a, m).await.unwrap();
        }
        for m in &msgs {
            assert_eq!(&read_frame(&mut b).await.unwrap(), m);
        }
    }

    #[tokio::test]
    async fn oversized_frame_header_is_rejected_without_allocating() {
        let (mut a, mut b) = tokio::io::duplex(64);
        a.write_u32((MAX_FRAME + 1) as u32).await.unwrap();
        let err = read_frame(&mut b).await.unwrap_err();
        assert!(matches!(err, CoreError::Frame(_)));
    }

    #[tokio::test]
    async fn garbage_frame_is_an_error_not_a_panic() {
        let (mut a, mut b) = tokio::io::duplex(64);
        a.write_u32(3).await.unwrap();
        a.write_all(&[0xff, 0xff, 0xff]).await.unwrap();
        assert!(read_frame(&mut b).await.is_err());
    }
}
