#![doc = include_str!(concat!(env!("OUT_DIR"), "/crdt_README.md"))]

mod body_doc;
mod body_doc_manager;
mod history;
mod memory_storage;
#[cfg(all(not(target_arch = "wasm32"), feature = "crdt-sqlite"))]
mod sqlite_storage;
mod storage;
mod sync;
mod sync_handler;
mod sync_manager;
mod types;
mod workspace_doc;

pub use body_doc::BodyDoc;
pub use body_doc_manager::BodyDocManager;
pub use history::{ChangeType, FileDiff, HistoryEntry, HistoryManager};
pub use memory_storage::MemoryStorage;
#[cfg(all(not(target_arch = "wasm32"), feature = "crdt-sqlite"))]
pub use sqlite_storage::SqliteStorage;
pub use storage::{CrdtStorage, StorageResult};
pub use sync::{
    BodySyncProtocol, SyncMessage, SyncProtocol, frame_body_message, unframe_body_message,
};
pub use sync_handler::{GuestConfig, SyncHandler};
pub use sync_manager::{BodySyncResult, RustSyncManager, SyncMessageResult};
pub use types::{BinaryRef, CrdtUpdate, FileMetadata, UpdateOrigin};
pub use workspace_doc::WorkspaceCrdt;
