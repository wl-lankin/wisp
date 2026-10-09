use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::io::{AsyncRead, AsyncWrite};
use uuid::Uuid;

use super::{Event, Inner, StreamKind};
use crate::protocol::write_value;
use crate::transfer::{receive_file, send_file, RESUME_WINDOW};
use crate::{CoreError, Result};

const RETRY_EVERY: Duration = Duration::from_secs(3);
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

fn due(last: &mut Option<Instant>, finished: bool) -> bool {
    if finished || last.is_none_or(|t| t.elapsed() >= PROGRESS_EVERY) {
        *last = Some(Instant::now());
        return true;
    }
    false
}

impl Inner {
    pub(super) async fn receive_on<S: AsyncRead + AsyncWrite + Unpin>(
        self: &Arc<Self>,
        peer: Uuid,
        mut stream: S,
    ) -> Result<()> {
        let transfer_id = Uuid::new_v4();
        let events = self.events.clone();
        let mut last = None;
        let mut progress = move |done: u64, total: u64| {
            if due(&mut last, done == total) {
                let _ = events.send(Event::TransferProgress {
                    transfer_id,
                    peer,
                    name: String::new(),
                    done,
                    total,
                    outgoing: false,
                });
            }
        };
        let path = receive_file(&mut stream, &self.save_dir, &mut progress).await?;
        self.emit(Event::TransferDone { transfer_id, peer, path: Some(path) });
        Ok(())
    }

    async fn send_one(self: &Arc<Self>, peer: Uuid, transfer_id: Uuid, path: &Path, name: &str) -> Result<()> {
        let conn = self.connection(peer).ok_or_else(|| CoreError::UnknownPeer(peer.to_string()))?;
        let (send, recv) = conn.open_bi().await.map_err(|e| CoreError::Transport(e.to_string()))?;
        let mut stream = tokio::io::join(recv, send);
        write_value(&mut stream, &StreamKind::File).await?;
        let total = tokio::fs::metadata(path).await?.len();
        let events = self.events.clone();
        let label = name.to_string();
        let mut last = None;
        let mut progress = move |done: u64| {
            if due(&mut last, done == total) {
                let _ = events.send(Event::TransferProgress {
                    transfer_id,
                    peer,
                    name: label.clone(),
                    done,
                    total,
                    outgoing: true,
                });
            }
        };
        send_file(&mut stream, transfer_id, path, name, &mut progress).await
    }

    /// Retries with the same transfer ID, so the receiver resumes from its partial file.
    pub(super) fn spawn_send(self: &Arc<Self>, peer: Uuid, path: PathBuf, name: String, remove_after: bool) -> Uuid {
        let transfer_id = Uuid::new_v4();
        let this = self.clone();
        tokio::spawn(async move {
            let mut first_failure: Option<Instant> = None;
            loop {
                if this.is_closed() {
                    this.emit(Event::TransferFailed { transfer_id, peer, reason: "shut down".into() });
                    break;
                }
                match this.send_one(peer, transfer_id, &path, &name).await {
                    Ok(()) => {
                        this.emit(Event::TransferDone { transfer_id, peer, path: None });
                        break;
                    }
                    Err(e) => {
                        let since = *first_failure.get_or_insert_with(Instant::now);
                        if since.elapsed() >= RESUME_WINDOW {
                            this.emit(Event::TransferFailed { transfer_id, peer, reason: e.to_string() });
                            break;
                        }
                        if since.elapsed() < RETRY_EVERY {
                            this.emit(Event::TransferPaused { transfer_id, peer });
                        }
                        tokio::time::sleep(RETRY_EVERY).await;
                    }
                }
            }
            if remove_after {
                let _ = tokio::fs::remove_file(&path).await;
            }
        });
        transfer_id
    }
}
