use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::io::{AsyncRead, AsyncWrite};
use uuid::Uuid;

use super::{Event, Inner, StreamKind};
use crate::protocol::write_value;
use crate::transfer::{receive_file, send_file, FileHeader, RESUME_WINDOW};
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

/// What a sender does after a failed attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Retry {
    /// A new drop: tell the user, then retry.
    Paused,
    /// Still within the resume window: retry quietly.
    Wait,
    /// The peer stayed away for the whole resume window.
    GiveUp,
}

/// The resume window runs from the first failure after the transfer last made progress, so a
/// transfer that keeps resuming is never cut off by an old drop.
#[derive(Debug, Default)]
struct ResumeClock {
    first_failure: Option<Instant>,
    furthest: u64,
}

impl ResumeClock {
    /// `reached` is how far the failed attempt got (the furthest offset it sent).
    fn on_failure(&mut self, reached: u64, now: Instant) -> Retry {
        if reached > self.furthest {
            self.furthest = reached;
            self.first_failure = None;
        }
        match self.first_failure {
            None => {
                self.first_failure = Some(now);
                Retry::Paused
            }
            Some(since) if now.saturating_duration_since(since) >= RESUME_WINDOW => Retry::GiveUp,
            Some(_) => Retry::Wait,
        }
    }
}

impl Inner {
    pub(super) async fn receive_on<S: AsyncRead + AsyncWrite + Unpin>(
        self: &Arc<Self>,
        peer: Uuid,
        mut stream: S,
    ) -> Result<()> {
        let mut header: Option<FileHeader> = None;
        let mut last = None;
        let result = {
            let events = &self.events;
            let mut progress = |h: &FileHeader, done: u64| {
                if header.is_none() {
                    header = Some(h.clone());
                }
                if due(&mut last, done == h.size) {
                    let _ = events.send(Event::TransferProgress {
                        transfer_id: h.transfer_id,
                        peer,
                        name: h.name.clone(),
                        done,
                        total: h.size,
                        outgoing: false,
                    });
                }
            };
            receive_file(&mut stream, &self.save_dir, &mut progress).await
        };
        match (result, header) {
            (Ok(path), Some(h)) => {
                self.emit(Event::TransferDone { transfer_id: h.transfer_id, peer, path: Some(path) });
                Ok(())
            }
            (Ok(_), None) => Ok(()),
            (Err(e), Some(h)) => {
                self.emit(Event::TransferFailed { transfer_id: h.transfer_id, peer, reason: e.to_string() });
                Err(e)
            }
            (Err(e), None) => Err(e),
        }
    }

    /// `reached` is set to the furthest offset this attempt sent, even when it fails.
    async fn send_one(
        self: &Arc<Self>,
        peer: Uuid,
        transfer_id: Uuid,
        path: &Path,
        name: &str,
        reached: &mut u64,
    ) -> Result<()> {
        let conn = self.connection(peer).ok_or_else(|| CoreError::UnknownPeer(peer.to_string()))?;
        let (send, recv) = conn.open_bi().await.map_err(|e| CoreError::Transport(e.to_string()))?;
        let mut stream = tokio::io::join(recv, send);
        write_value(&mut stream, &StreamKind::File).await?;
        let total = tokio::fs::metadata(path).await?.len();
        let events = &self.events;
        let mut last = None;
        let mut progress = |done: u64| {
            *reached = done;
            if due(&mut last, done == total) {
                let _ = events.send(Event::TransferProgress {
                    transfer_id,
                    peer,
                    name: name.to_string(),
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
            let mut clock = ResumeClock::default();
            loop {
                if this.is_closed() {
                    this.emit(Event::TransferFailed { transfer_id, peer, reason: "shut down".into() });
                    break;
                }
                let mut reached = 0;
                match this.send_one(peer, transfer_id, &path, &name, &mut reached).await {
                    Ok(()) => {
                        this.emit(Event::TransferDone { transfer_id, peer, path: None });
                        break;
                    }
                    Err(e) => match clock.on_failure(reached, Instant::now()) {
                        Retry::GiveUp => {
                            this.emit(Event::TransferFailed { transfer_id, peer, reason: e.to_string() });
                            break;
                        }
                        Retry::Paused => this.emit(Event::TransferPaused { transfer_id, peer }),
                        Retry::Wait => {}
                    },
                }
                tokio::time::sleep(RETRY_EVERY).await;
            }
            if remove_after {
                let _ = tokio::fs::remove_file(&path).await;
            }
        });
        transfer_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resume_window_restarts_whenever_a_transfer_made_progress() {
        let t0 = Instant::now();
        let at = |mins: u64| t0 + Duration::from_secs(mins * 60);
        let mut clock = ResumeClock::default();
        assert_eq!(clock.on_failure(1_000, at(0)), Retry::Paused);
        assert_eq!(clock.on_failure(1_000, at(5)), Retry::Wait, "no progress, same drop");
        // The peer came back and the transfer moved on before it dropped again.
        assert_eq!(clock.on_failure(5_000, at(9)), Retry::Paused, "a new drop is reported again");
        assert_eq!(clock.on_failure(5_000, at(18)), Retry::Wait, "the window counts from the new drop");
        assert_eq!(clock.on_failure(5_000, at(19)), Retry::GiveUp);
    }

    #[test]
    fn a_peer_that_never_answers_gives_up_after_the_window() {
        let t0 = Instant::now();
        let mut clock = ResumeClock::default();
        assert_eq!(clock.on_failure(0, t0), Retry::Paused);
        assert_eq!(clock.on_failure(0, t0 + RESUME_WINDOW - Duration::from_secs(1)), Retry::Wait);
        assert_eq!(clock.on_failure(0, t0 + RESUME_WINDOW), Retry::GiveUp);
    }
}
