//! CRDT-based synchronization for Diaryx workspaces.
//!
//! This module provides conflict-free replicated data types (CRDTs) for
//! synchronizing workspace metadata and document content across multiple
//! clients. It uses the [yrs](https://docs.rs/yrs) crate (Rust port of Yjs)
//! for CRDT operations.
//!
//! # Architecture
//!
//! The CRDT system has several layers:
//!
//! 1. **Types** ([`types`]): Core data structures like [`FileMetadata`] and [`BinaryRef`]
//! 2. **Storage** ([`storage`]): Abstraction for persisting CRDT state to SQLite/memory
//! 3. **Workspace CRDT** ([`WorkspaceCrdt`]): Y.Doc for workspace file hierarchy
//! 4. **Body CRDT** ([`BodyDoc`]): Per-file Y.Doc for document content
//! 5. **Sync Protocol** ([`SyncProtocol`]): Y-sync for Hocuspocus server communication
//! 6. **History** ([`HistoryManager`]): Version history and time-travel
//!
//! # Feature Flag
//!
//! This module is only available when the `crdt` feature is enabled:
//!
//! ```toml
//! [dependencies]
//! diaryx_core = { version = "...", features = ["crdt"] }
//! ```
//!
//! # Example
//!
//! ```ignore
//! use diaryx_core::crdt::{MemoryStorage, CrdtStorage, FileMetadata};
//!
//! let storage = MemoryStorage::new();
//!
//! // Save some CRDT state
//! storage.save_doc("workspace", b"binary crdt state")?;
//!
//! // Track an update
//! let update_id = storage.append_update(
//!     "workspace",
//!     b"incremental update",
//!     UpdateOrigin::Local,
//! )?;
//! ```

mod body_doc;
mod body_doc_manager;
mod history;
mod memory_storage;
#[cfg(all(not(target_arch = "wasm32"), feature = "crdt-sqlite"))]
mod sqlite_storage;
mod storage;
mod sync;
mod types;
mod workspace_doc;

pub use body_doc::BodyDoc;
pub use body_doc_manager::BodyDocManager;
pub use history::{ChangeType, FileDiff, HistoryEntry, HistoryManager};
pub use memory_storage::MemoryStorage;
#[cfg(all(not(target_arch = "wasm32"), feature = "crdt-sqlite"))]
pub use sqlite_storage::SqliteStorage;
pub use storage::{CrdtStorage, StorageResult};
pub use sync::{BodySyncProtocol, SyncMessage, SyncProtocol};
pub use types::{BinaryRef, CrdtUpdate, FileMetadata, UpdateOrigin};
pub use workspace_doc::WorkspaceCrdt;
