//! Callback registry for filesystem event subscriptions.
//!
//! This module provides a thread-safe registry for managing event callbacks.
//! Subscribers receive [`FileSystemEvent`](super::FileSystemEvent) notifications
//! when filesystem operations occur.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use super::events::FileSystemEvent;

/// A unique identifier for a subscription.
pub type SubscriptionId = u64;

/// Callback function type for filesystem events.
///
/// Callbacks receive a reference to the event and should not block for extended periods.
pub type EventCallback = Arc<dyn Fn(&FileSystemEvent) + Send + Sync>;

/// Thread-safe registry for managing event subscriptions.
///
/// The registry supports:
/// - Subscribing to events with unique IDs
/// - Unsubscribing by ID
/// - Emitting events to all active subscribers
///
/// # Example
///
/// ```ignore
/// use diaryx_core::fs::{CallbackRegistry, FileSystemEvent};
/// use std::sync::Arc;
///
/// let registry = CallbackRegistry::new();
///
/// let id = registry.subscribe(Arc::new(|event| {
///     println!("Event: {:?}", event);
/// }));
///
/// registry.emit(&FileSystemEvent::file_created("test.md".into()));
///
/// registry.unsubscribe(id);
/// ```
pub struct CallbackRegistry {
    /// Map of subscription IDs to callbacks.
    callbacks: RwLock<HashMap<SubscriptionId, EventCallback>>,
    /// Counter for generating unique subscription IDs.
    next_id: AtomicU64,
}

impl CallbackRegistry {
    /// Create a new empty callback registry.
    pub fn new() -> Self {
        Self {
            callbacks: RwLock::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        }
    }

    /// Subscribe to filesystem events.
    ///
    /// Returns a subscription ID that can be used to unsubscribe later.
    pub fn subscribe(&self, callback: EventCallback) -> SubscriptionId {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let mut callbacks = self.callbacks.write().unwrap();
        callbacks.insert(id, callback);
        id
    }

    /// Unsubscribe from filesystem events.
    ///
    /// Returns `true` if the subscription was found and removed.
    pub fn unsubscribe(&self, id: SubscriptionId) -> bool {
        let mut callbacks = self.callbacks.write().unwrap();
        callbacks.remove(&id).is_some()
    }

    /// Emit an event to all registered callbacks.
    ///
    /// Callbacks are invoked synchronously in an undefined order.
    /// If a callback panics, it does not affect other callbacks.
    pub fn emit(&self, event: &FileSystemEvent) {
        let callbacks = self.callbacks.read().unwrap();
        for callback in callbacks.values() {
            // Use catch_unwind to prevent one callback from breaking others
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                callback(event);
            }));
        }
    }

    /// Get the number of active subscriptions.
    pub fn subscriber_count(&self) -> usize {
        let callbacks = self.callbacks.read().unwrap();
        callbacks.len()
    }

    /// Check if there are any active subscriptions.
    pub fn has_subscribers(&self) -> bool {
        let callbacks = self.callbacks.read().unwrap();
        !callbacks.is_empty()
    }

    /// Clear all subscriptions.
    pub fn clear(&self) {
        let mut callbacks = self.callbacks.write().unwrap();
        callbacks.clear();
    }
}

impl Default for CallbackRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for CallbackRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let callbacks = self.callbacks.read().unwrap();
        f.debug_struct("CallbackRegistry")
            .field("subscriber_count", &callbacks.len())
            .field("next_id", &self.next_id.load(Ordering::SeqCst))
            .finish()
    }
}

// CallbackRegistry is Send + Sync because:
// - RwLock<HashMap<...>> is Send + Sync when contents are Send + Sync
// - AtomicU64 is Send + Sync
// - EventCallback = Arc<dyn Fn + Send + Sync> is Send + Sync
unsafe impl Send for CallbackRegistry {}
unsafe impl Sync for CallbackRegistry {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn test_subscribe_and_emit() {
        let registry = CallbackRegistry::new();
        let counter = Arc::new(AtomicUsize::new(0));

        let counter_clone = Arc::clone(&counter);
        let _id = registry.subscribe(Arc::new(move |_event| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        }));

        assert_eq!(registry.subscriber_count(), 1);

        let event = FileSystemEvent::file_created(PathBuf::from("test.md"));
        registry.emit(&event);

        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_unsubscribe() {
        let registry = CallbackRegistry::new();
        let counter = Arc::new(AtomicUsize::new(0));

        let counter_clone = Arc::clone(&counter);
        let id = registry.subscribe(Arc::new(move |_event| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        }));

        assert_eq!(registry.subscriber_count(), 1);

        let result = registry.unsubscribe(id);
        assert!(result);
        assert_eq!(registry.subscriber_count(), 0);

        let event = FileSystemEvent::file_created(PathBuf::from("test.md"));
        registry.emit(&event);

        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_unsubscribe_nonexistent() {
        let registry = CallbackRegistry::new();
        let result = registry.unsubscribe(999);
        assert!(!result);
    }

    #[test]
    fn test_multiple_subscribers() {
        let registry = CallbackRegistry::new();
        let counter1 = Arc::new(AtomicUsize::new(0));
        let counter2 = Arc::new(AtomicUsize::new(0));

        let c1 = Arc::clone(&counter1);
        registry.subscribe(Arc::new(move |_event| {
            c1.fetch_add(1, Ordering::SeqCst);
        }));

        let c2 = Arc::clone(&counter2);
        registry.subscribe(Arc::new(move |_event| {
            c2.fetch_add(1, Ordering::SeqCst);
        }));

        assert_eq!(registry.subscriber_count(), 2);

        let event = FileSystemEvent::file_created(PathBuf::from("test.md"));
        registry.emit(&event);

        assert_eq!(counter1.load(Ordering::SeqCst), 1);
        assert_eq!(counter2.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_unique_subscription_ids() {
        let registry = CallbackRegistry::new();

        let id1 = registry.subscribe(Arc::new(|_| {}));
        let id2 = registry.subscribe(Arc::new(|_| {}));
        let id3 = registry.subscribe(Arc::new(|_| {}));

        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_clear() {
        let registry = CallbackRegistry::new();

        registry.subscribe(Arc::new(|_| {}));
        registry.subscribe(Arc::new(|_| {}));

        assert_eq!(registry.subscriber_count(), 2);

        registry.clear();

        assert_eq!(registry.subscriber_count(), 0);
        assert!(!registry.has_subscribers());
    }

    #[test]
    fn test_callback_panic_isolation() {
        let registry = CallbackRegistry::new();
        let counter = Arc::new(AtomicUsize::new(0));

        // First callback panics
        registry.subscribe(Arc::new(|_| {
            panic!("Test panic");
        }));

        // Second callback should still be called
        let counter_clone = Arc::clone(&counter);
        registry.subscribe(Arc::new(move |_| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        }));

        let event = FileSystemEvent::file_created(PathBuf::from("test.md"));
        registry.emit(&event);

        // The second callback should have been called despite the first panicking
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }
}
