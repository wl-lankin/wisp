use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use uuid::Uuid;

use super::{Event, Inner};
use crate::discovery::{Advert, Discovered, Discovery, DiscoveryEvent};
use crate::identity::cap_device_name;
use crate::Result;

impl Inner {
    pub(super) fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    /// Resolves once the node is shut down (right away if it already is).
    pub(super) async fn closed_signal(&self) {
        let mut rx = self.shutdown.subscribe();
        let _ = rx.wait_for(|closed| *closed).await;
    }

    /// Periodically dials every paired peer that is not connected, using remembered and
    /// discovered addresses. Both sides dial; `register` keeps one connection.
    pub(super) fn spawn_dialer(self: Arc<Self>, every: Duration) {
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(every);
            loop {
                tokio::select! {
                    biased;
                    _ = self.closed_signal() => break,
                    _ = ticker.tick() => {}
                }
                let offline: Vec<Uuid> = {
                    let peers = self.peers.lock().unwrap();
                    let conns = self.conns.lock().unwrap();
                    peers.all().iter().map(|p| p.device_id).filter(|id| !conns.contains_key(id)).collect()
                };
                for peer in offline {
                    self.clone().dial(peer);
                }
            }
        });
    }

    pub(super) fn dial(self: Arc<Self>, peer: Uuid) {
        if self.is_closed() || !self.dialing.lock().unwrap().insert(peer) {
            return;
        }
        let target = {
            let peers = self.peers.lock().unwrap();
            let discovered = self.discovered.lock().unwrap();
            peers.get(peer).map(|p| {
                let mut addrs = p.addresses.clone();
                if let Some(d) = discovered.get(peer) {
                    for a in &d.addresses {
                        if !addrs.contains(a) {
                            addrs.push(*a);
                        }
                    }
                }
                (p.fingerprint, addrs)
            })
        };
        tokio::spawn(async move {
            if let Some((fp, addrs)) = target {
                if !addrs.is_empty() {
                    if let Ok((conn, _)) = self.transport.connect_any(&addrs, Some(fp)).await {
                        self.register(peer, conn, true);
                    }
                }
            }
            self.dialing.lock().unwrap().remove(&peer);
        });
    }

    pub(super) fn start_discovery(self: &Arc<Self>) -> Result<()> {
        let advert = Advert {
            device_id: self.identity.device_id,
            name: cap_device_name(&self.identity.name),
            os: self.identity.os,
            port: self.transport.local_addr()?.port(),
            fingerprint: self.identity.fingerprint(),
        };
        let (discovery, mut rx) = Discovery::start(&advert)?;
        *self.discovery.lock().unwrap() = Some(discovery);
        let this = self.clone();
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                match event {
                    DiscoveryEvent::Found(d) => this.on_discovered(d),
                    DiscoveryEvent::Lost(id) => this.discovered.lock().unwrap().remove(id),
                }
            }
        });
        Ok(())
    }

    fn on_discovered(&self, d: Discovered) {
        let pinned = self.peers.lock().unwrap().get(d.device_id).map(|p| p.fingerprint);
        match pinned {
            Some(fp) if fp != d.fingerprint => self.emit(Event::IdentityChanged { peer: d.device_id }),
            Some(_) => {
                self.discovered.lock().unwrap().merge(d);
            }
            None => {
                let (device, name, os, addresses) = (d.device_id, d.name.clone(), d.os, d.addresses.clone());
                if self.discovered.lock().unwrap().merge(d) {
                    self.emit(Event::Discovered { device, name, os, addresses });
                }
            }
        }
    }
}
