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
//! 3. **Workspace CRDT** (coming soon): Y.Doc for workspace file hierarchy
//! 4. **Body CRDT** (coming soon): Per-file Y.Doc for document content
//! 5. **Sync Protocol** (coming soon): Y-sync for Hocuspocus server communication
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

mod memory_storage;
mod storage;
mod types;

pub use memory_storage::MemoryStorage;
pub use storage::{CrdtStorage, StorageResult};
pub use types::{BinaryRef, CrdtUpdate, FileMetadata, UpdateOrigin};
