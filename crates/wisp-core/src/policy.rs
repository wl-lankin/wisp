use std::collections::{HashMap, VecDeque};
use std::time::Instant;

use uuid::Uuid;

use crate::item::{ClipItem, ItemKind, Preview, MAX_CLIP_BYTES};
use crate::peers::DeliveryMode;
use crate::protocol::Message;

pub const OFFLINE_QUEUE_CAP: usize = 20;

pub fn offer_for(item: &ClipItem, live: bool) -> Message {
    let size = item.payload.size();
    let (kind, preview) = if size > MAX_CLIP_BYTES {
        let ext = match item.payload.kind() {
            ItemKind::Image => "png",
            ItemKind::Html => "html",
            _ => "txt",
        };
        (ItemKind::File, Preview::File { name: format!("clipboard-{}.{ext}", item.created_ms) })
    } else {
        (item.payload.kind(), item.payload.preview())
    };
    Message::Offer { item_id: item.id, kind, size, preview, live }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfferAction {
    AutoPull,
    Inbox,
}

/// Decided by the receiver: `mode` is how items from the sending peer reach this device.
pub fn on_offer(mode: DeliveryMode, kind: ItemKind, live: bool) -> OfferAction {
    if mode == DeliveryMode::AutoSync && live && kind != ItemKind::File {
        OfferAction::AutoPull
    } else {
        OfferAction::Inbox
    }
}

#[derive(Default)]
pub struct OfferQueue {
    queues: HashMap<Uuid, VecDeque<Message>>,
}

impl OfferQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replayed offers are not live, so an auto-sync receiver shows them instead of
    /// overwriting its clipboard with something stale.
    pub fn push(&mut self, peer: Uuid, item: &ClipItem) {
        let q = self.queues.entry(peer).or_default();
        q.push_back(offer_for(item, false));
        while q.len() > OFFLINE_QUEUE_CAP {
            q.pop_front();
        }
    }

    pub fn drain(&mut self, peer: Uuid) -> Vec<Message> {
        self.queues.remove(&peer).map(Vec::from).unwrap_or_default()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct InboxEntry {
    pub peer: Uuid,
    pub item_id: Uuid,
    pub kind: ItemKind,
    pub size: u64,
    pub preview: Preview,
    pub received: Instant,
}

#[derive(Default)]
pub struct Inbox {
    entries: Vec<InboxEntry>,
}

impl Inbox {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, entry: InboxEntry) {
        self.entries.retain(|e| e.item_id != entry.item_id);
        self.entries.push(entry);
    }

    pub fn latest(&self) -> Option<&InboxEntry> {
        self.entries.last()
    }

    pub fn take(&mut self, item_id: Uuid) -> Option<InboxEntry> {
        let i = self.entries.iter().position(|e| e.item_id == item_id)?;
        Some(self.entries.remove(i))
    }

    pub fn dismiss(&mut self, item_id: Uuid) {
        self.entries.retain(|e| e.item_id != item_id);
    }

    pub fn all(&self) -> Vec<&InboxEntry> {
        self.entries.iter().rev().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::ClipPayload;

    fn text_item(s: &str) -> ClipItem {
        ClipItem::new(Uuid::new_v4(), ClipPayload::Text(s.into()))
    }

    #[test]
    fn autosync_pulls_live_clipboard_items_only() {
        use DeliveryMode::*;
        assert_eq!(on_offer(AutoSync, ItemKind::Image, true), OfferAction::AutoPull);
        assert_eq!(on_offer(AutoSync, ItemKind::Text, false), OfferAction::Inbox);
        assert_eq!(on_offer(AutoSync, ItemKind::File, true), OfferAction::Inbox);
        assert_eq!(on_offer(Catch, ItemKind::Text, true), OfferAction::Inbox);
    }

    #[test]
    fn offer_carries_preview_and_live_flag() {
        let item = text_item("hello");
        match offer_for(&item, true) {
            Message::Offer { item_id, kind, live, preview, .. } => {
                assert_eq!(item_id, item.id);
                assert_eq!(kind, ItemKind::Text);
                assert!(live);
                assert_eq!(preview, Preview::Text("hello".into()));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn oversized_item_is_always_a_file_offer() {
        let big = ClipPayload::Text("x".repeat(MAX_CLIP_BYTES as usize + 1));
        let item = ClipItem::new(Uuid::new_v4(), big);
        match offer_for(&item, true) {
            Message::Offer { kind, preview, .. } => {
                assert_eq!(kind, ItemKind::File);
                assert!(matches!(preview, Preview::File { .. }));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn offline_queue_keeps_latest_twenty_offers_per_peer() {
        let mut q = OfferQueue::new();
        let peer = Uuid::new_v4();
        let other = Uuid::new_v4();
        let mut last_id = Uuid::nil();
        for i in 0..25 {
            let item = text_item(&format!("item {i}"));
            last_id = item.id;
            q.push(peer, &item);
        }
        q.push(other, &text_item("other"));
        let drained = q.drain(peer);
        assert_eq!(drained.len(), OFFLINE_QUEUE_CAP);
        assert!(drained.iter().all(|m| matches!(m, Message::Offer { live: false, .. })));
        assert!(matches!(drained.last(), Some(Message::Offer { item_id, .. }) if *item_id == last_id));
        assert!(q.drain(peer).is_empty());
        assert_eq!(q.drain(other).len(), 1);
    }

    #[test]
    fn inbox_latest_take_and_dismiss() {
        let mut inbox = Inbox::new();
        let peer = Uuid::new_v4();
        let entry = |id| InboxEntry {
            peer,
            item_id: id,
            kind: ItemKind::Text,
            size: 1,
            preview: Preview::Text("x".into()),
            received: Instant::now(),
        };
        let (a, b, c) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        inbox.add(entry(a));
        inbox.add(entry(b));
        inbox.add(entry(c));
        assert_eq!(inbox.latest().unwrap().item_id, c);
        assert_eq!(inbox.take(c).unwrap().item_id, c);
        inbox.dismiss(b);
        assert_eq!(inbox.latest().unwrap().item_id, a);
        assert_eq!(inbox.all().len(), 1);
        assert!(inbox.take(c).is_none());
    }
}
