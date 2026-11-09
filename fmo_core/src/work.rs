//! Commit notifications wake workers; durable queue rows remain authoritative.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::sync::watch;

#[derive(Clone, Default, Debug)]
pub(crate) struct WorkNotifications(Arc<Mutex<HashMap<String, watch::Sender<u64>>>>);

impl WorkNotifications {
    pub(crate) fn subscribe(&self, topic: &str) -> watch::Receiver<u64> {
        self.0
            .lock()
            .expect("not poisoned")
            .entry(topic.to_owned())
            .or_insert_with(|| watch::channel(0).0)
            .subscribe()
    }

    pub(crate) fn notify(&self, topic: &str) {
        if let Some(sender) = self.0.lock().expect("not poisoned").get(topic) {
            sender.send_modify(|generation| *generation = generation.wrapping_add(1));
        }
    }
}

#[cfg(test)]
mod tests;
