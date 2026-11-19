use std::collections::HashMap;
use std::hash::Hash;
use std::sync::{Arc, Mutex, PoisonError};

/// One async mutex per key, created on first use and dropped with the last
/// guard or waiter, so the map only ever holds keys in use.
pub(crate) struct KeyedLocks<K> {
    slots: Mutex<HashMap<K, Slot>>,
}

struct Slot {
    lock: Arc<tokio::sync::Mutex<()>>,
    users: usize,
}

impl<K> Default for KeyedLocks<K> {
    fn default() -> Self {
        Self {
            slots: Mutex::new(HashMap::new()),
        }
    }
}

impl<K: Eq + Hash + Clone> KeyedLocks<K> {
    pub(crate) async fn lock(&self, key: K) -> KeyGuard<'_, K> {
        let lock = {
            let mut slots = self.slots();
            let slot = slots.entry(key.clone()).or_insert_with(|| Slot {
                lock: Arc::default(),
                users: 0,
            });
            slot.users += 1;
            slot.lock.clone()
        };
        let claim = Claim { locks: self, key };
        KeyGuard {
            _held: lock.lock_owned().await,
            _claim: claim,
        }
    }

    fn slots(&self) -> std::sync::MutexGuard<'_, HashMap<K, Slot>> {
        self.slots.lock().unwrap_or_else(PoisonError::into_inner)
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.slots().len()
    }
}

/// Field order is drop order: the lock is released before the claim can
/// remove its slot, so a newcomer never gets a fresh lock while this one is
/// still held.
pub(crate) struct KeyGuard<'a, K: Eq + Hash + Clone> {
    _held: tokio::sync::OwnedMutexGuard<()>,
    _claim: Claim<'a, K>,
}

/// One holder's or waiter's share of a slot; a waiter whose future is
/// dropped before it gets the lock gives its share back too.
struct Claim<'a, K: Eq + Hash + Clone> {
    locks: &'a KeyedLocks<K>,
    key: K,
}

impl<K: Eq + Hash + Clone> Drop for Claim<'_, K> {
    fn drop(&mut self) {
        let mut slots = self.locks.slots();
        if let Some(slot) = slots.get_mut(&self.key) {
            slot.users -= 1;
            if slot.users == 0 {
                slots.remove(&self.key);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn one_key_is_exclusive_and_others_are_not() {
        let locks = KeyedLocks::default();
        let a = locks.lock("a").await;
        let _b = locks.lock("b").await;
        assert!(
            tokio::time::timeout(Duration::from_millis(50), locks.lock("a"))
                .await
                .is_err(),
            "a second holder of `a`"
        );
        drop(a);
        let _a = locks.lock("a").await;
    }

    #[tokio::test]
    async fn the_map_empties_once_every_holder_and_waiter_is_gone() {
        let locks = KeyedLocks::default();
        let held = locks.lock("k").await;
        let waiting = tokio::time::timeout(Duration::from_millis(20), locks.lock("k")).await;
        assert!(waiting.is_err(), "the waiter was cancelled while waiting");
        assert_eq!(locks.len(), 1);
        drop(held);
        assert_eq!(locks.len(), 0);

        let first = locks.lock("k").await;
        let (second, ()) = tokio::join!(locks.lock("k"), async { drop(first) });
        assert_eq!(locks.len(), 1);
        drop(second);
        assert_eq!(locks.len(), 0);
    }
}
