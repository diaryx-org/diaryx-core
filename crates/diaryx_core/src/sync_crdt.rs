//! Live sync module for CRDT-based synchronization.
//!
//! This module provides cross-platform abstractions for real-time collaborative
//! editing using Conflict-Free Replicated Data Types (CRDTs) via automerge-rs.
//!
//! # Architecture
//!
//! - **LiveSyncProvider**: Platform-specific transport layer (WebSocket, WebRTC, etc.)
//! - **DocumentSync**: Manages automerge state for a single markdown file
//! - **SyncManager**: Coordinates syncing across all files in a workspace
//!
//! # Storage Strategy
//!
//! CRDT documents are kept in memory only:
//! 1. On startup, load markdown files and create fresh automerge documents
//! 2. During editing, all changes go through automerge (maintains CRDT state)
//! 3. Periodically persist markdown content to disk
//! 4. Exchange sync messages with peers via LiveSyncProvider
//! 5. On shutdown, markdown is already saved; discard automerge state

use crate::fs::FileSystem;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

#[cfg(feature = "live-sync")]
use automerge::{AutoCommit, ReadDoc, transaction::Transactable, sync::SyncDoc};

/// Errors that can occur during sync operations.
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    /// Transport-level error (network, connection, etc.)
    #[error("Transport error: {0}")]
    Transport(String),

    /// Error serializing or deserializing automerge data
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Error reading or writing files
    #[error("Filesystem error: {0}")]
    Filesystem(#[from] std::io::Error),

    /// Automerge-specific error
    #[cfg(feature = "live-sync")]
    #[error("Automerge error: {0}")]
    Automerge(#[from] automerge::AutomergeError),

    /// Peer connection error
    #[error("Peer error: {0}")]
    Peer(String),

    /// Generic error
    #[error("{0}")]
    Other(String),
}

/// Unique identifier for a peer in the sync network.
pub type PeerId = String;

/// Cross-platform trait for live sync transport providers.
///
/// Platform-specific implementations handle the transport layer
/// (WebSocket, WebRTC, etc.) for sending and receiving sync messages.
///
/// # Example Implementations
///
/// - `WebSocketSyncProvider` (Tauri): Uses WebSocket to connect to a relay server
/// - `WebRtcSyncProvider` (Future): Direct P2P using WebRTC
pub trait LiveSyncProvider: Send + Sync {
    /// Initialize connection to the sync provider.
    ///
    /// This should establish the connection but may not block until
    /// fully connected. Use `is_connected()` to check connection status.
    fn connect(&mut self) -> Result<(), SyncError>;

    /// Disconnect from the sync provider.
    ///
    /// Should gracefully close connections and clean up resources.
    fn disconnect(&mut self) -> Result<(), SyncError>;

    /// Send a sync message to peers.
    ///
    /// The message is an opaque byte array containing automerge sync data.
    /// The provider should broadcast this to all peers in the same workspace.
    fn send_sync_message(&self, message: Vec<u8>) -> Result<(), SyncError>;

    /// Receive pending sync messages from peers.
    ///
    /// Returns a list of messages that have arrived since the last call.
    /// Each message is from a single peer and contains automerge sync data.
    fn receive_sync_messages(&self) -> Result<Vec<Vec<u8>>, SyncError>;

    /// Check if currently connected to the sync provider.
    fn is_connected(&self) -> bool;

    /// Get the current peer ID.
    ///
    /// Returns None if not connected.
    fn peer_id(&self) -> Option<PeerId>;
}

#[cfg(feature = "live-sync")]
/// Sync state for a single document (markdown file).
///
/// Manages the automerge document and per-peer sync state.
pub struct DocumentSync {
    /// Path to the file being synced (relative to workspace)
    pub path: PathBuf,

    /// Automerge document containing the markdown content
    pub doc: AutoCommit,

    /// Sync state for each peer we're syncing with
    pub peer_states: HashMap<PeerId, automerge::sync::State>,

    /// Last known content (for change detection)
    last_content: String,
}

#[cfg(feature = "live-sync")]
impl DocumentSync {
    /// Create a new DocumentSync from a markdown file.
    ///
    /// Initializes an automerge document with the file's content.
    pub fn new(path: PathBuf, content: String) -> Result<Self, SyncError> {
        let mut doc = AutoCommit::new();

        // Initialize document with markdown content as a single text field
        doc.put(automerge::ROOT, "content", content.clone())?;

        Ok(Self {
            path,
            doc,
            peer_states: HashMap::new(),
            last_content: content,
        })
    }

    /// Get the current markdown content from the automerge document.
    pub fn get_content(&self) -> Result<String, SyncError> {
        if let Some((value, _)) = self.doc.get(automerge::ROOT, "content")? {
            value
                .to_str()
                .ok_or_else(|| SyncError::Other("Content is not a string".to_string()))
                .map(|s| s.to_string())
        } else {
            Err(SyncError::Other("Content field not found".to_string()))
        }
    }

    /// Update the markdown content in the automerge document.
    ///
    /// This should be called when the user makes local edits.
    pub fn update_content(&mut self, new_content: String) -> Result<(), SyncError> {
        self.doc.put(automerge::ROOT, "content", new_content.clone())?;
        self.last_content = new_content;
        Ok(())
    }

    /// Generate a sync message for a specific peer.
    ///
    /// Returns the sync message to send to the peer, or None if nothing to sync.
    pub fn generate_sync_message(&mut self, peer_id: &PeerId) -> Result<Option<Vec<u8>>, SyncError> {
        let state = self.peer_states.entry(peer_id.clone()).or_insert_with(automerge::sync::State::new);

        let message = self.doc.sync().generate_sync_message(state);
        Ok(message.map(|m| m.encode()))
    }

    /// Process a sync message received from a peer.
    ///
    /// Updates the document based on the peer's changes and returns
    /// whether the content changed.
    pub fn receive_sync_message(
        &mut self,
        peer_id: &PeerId,
        message: &[u8],
    ) -> Result<bool, SyncError> {
        let state = self.peer_states.entry(peer_id.clone()).or_insert_with(automerge::sync::State::new);

        let sync_message = automerge::sync::Message::decode(message)
            .map_err(|e| SyncError::Serialization(e.to_string()))?;

        self.doc.sync().receive_sync_message(state, sync_message)?;

        // Check if content changed
        let new_content = self.get_content()?;
        let changed = new_content != self.last_content;
        if changed {
            self.last_content = new_content;
        }

        Ok(changed)
    }

    /// Check if this document has changes to sync.
    pub fn has_pending_changes(&self) -> bool {
        // For simplicity, we'll always try to sync
        // A more sophisticated implementation could track this
        true
    }
}

#[cfg(feature = "live-sync")]
/// Manager for syncing multiple documents in a workspace.
///
/// Coordinates syncing across all markdown files, handling the
/// sync protocol and delegating transport to a LiveSyncProvider.
pub struct SyncManager {
    /// Sync provider (platform-specific transport)
    provider: Box<dyn LiveSyncProvider>,

    /// All documents being synced (path -> DocumentSync)
    documents: HashMap<PathBuf, DocumentSync>,

    /// Filesystem for reading/writing files
    filesystem: Arc<dyn FileSystem>,

    /// Workspace root path
    workspace_path: PathBuf,
}

#[cfg(feature = "live-sync")]
impl SyncManager {
    /// Create a new SyncManager.
    ///
    /// # Arguments
    ///
    /// * `provider` - Platform-specific sync provider
    /// * `filesystem` - Filesystem implementation
    /// * `workspace_path` - Root path of the workspace
    pub fn new(
        provider: Box<dyn LiveSyncProvider>,
        filesystem: Arc<dyn FileSystem>,
        workspace_path: PathBuf,
    ) -> Self {
        Self {
            provider,
            documents: HashMap::new(),
            filesystem,
            workspace_path,
        }
    }

    /// Start syncing the workspace.
    ///
    /// Loads all markdown files and initializes automerge documents.
    pub fn start(&mut self) -> Result<(), SyncError> {
        // Connect to sync provider
        self.provider.connect()?;

        // Load all markdown files
        let md_files = self
            .filesystem
            .list_md_files_recursive(&self.workspace_path)?;

        for file_path in md_files {
            let content = self.filesystem.read_to_string(&file_path)?;
            let relative_path = file_path
                .strip_prefix(&self.workspace_path)
                .map_err(|e| SyncError::Other(format!("Path error: {}", e)))?
                .to_path_buf();

            let doc_sync = DocumentSync::new(relative_path.clone(), content)?;
            self.documents.insert(relative_path, doc_sync);
        }

        Ok(())
    }

    /// Stop syncing and disconnect.
    pub fn stop(&mut self) -> Result<(), SyncError> {
        self.provider.disconnect()?;
        self.documents.clear();
        Ok(())
    }

    /// Perform a sync round.
    ///
    /// This should be called periodically (e.g., every 100ms) to:
    /// 1. Receive messages from peers
    /// 2. Generate and send sync messages
    /// 3. Write changed documents to disk
    /// 4. Discover and create missing files from synced content
    pub fn sync_round(&mut self) -> Result<SyncStats, SyncError> {
        let mut stats = SyncStats::default();

        if !self.provider.is_connected() {
            return Ok(stats);
        }

        // Receive messages from peers
        let messages = self.provider.receive_sync_messages()?;
        stats.messages_received = messages.len();

        for message in messages {
            // For simplicity, we'll apply each message to all documents
            // In a real implementation, messages would include document identifiers
            for doc_sync in self.documents.values_mut() {
                // TODO: Extract peer ID and document path from message
                // For now, use a placeholder peer ID
                let peer_id = "peer".to_string();

                if let Ok(changed) = doc_sync.receive_sync_message(&peer_id, &message) {
                    if changed {
                        stats.documents_updated += 1;
                        // Write to disk
                        let full_path = self.workspace_path.join(&doc_sync.path);
                        if let Ok(content) = doc_sync.get_content() {
                            let _ = self.filesystem.write_file(&full_path, &content);
                        }
                    }
                }
            }
        }

        // Discover and create missing files from synced documents
        // This handles the case where a new peer doesn't have files that exist on other peers
        if stats.documents_updated > 0 {
            if let Ok(created) = self.discover_and_create_missing_files() {
                stats.files_discovered = created;
            }
        }

        // Generate and send sync messages for documents with changes
        for (_, doc_sync) in &mut self.documents {
            if doc_sync.has_pending_changes() {
                // TODO: Support multiple peers
                let peer_id = "peer".to_string();
                if let Ok(Some(message)) = doc_sync.generate_sync_message(&peer_id) {
                    self.provider.send_sync_message(message)?;
                    stats.messages_sent += 1;
                }
            }
        }

        Ok(stats)
    }

    /// Update a document with local changes.
    ///
    /// Call this when the user edits a file locally.
    pub fn update_document(&mut self, path: &PathBuf, new_content: String) -> Result<(), SyncError> {
        let relative_path = path
            .strip_prefix(&self.workspace_path)
            .map_err(|e| SyncError::Other(format!("Path error: {}", e)))?
            .to_path_buf();

        if let Some(doc_sync) = self.documents.get_mut(&relative_path) {
            doc_sync.update_content(new_content)?;
        } else {
            // New file - create DocumentSync
            let doc_sync = DocumentSync::new(relative_path.clone(), new_content)?;
            self.documents.insert(relative_path, doc_sync);
        }

        Ok(())
    }

    /// Get the number of documents being synced.
    pub fn document_count(&self) -> usize {
        self.documents.len()
    }

    /// Check if a document is being synced.
    pub fn is_syncing(&self, path: &PathBuf) -> bool {
        let relative_path = path.strip_prefix(&self.workspace_path).ok();
        relative_path.map(|p| self.documents.contains_key(p)).unwrap_or(false)
    }

    /// Discover and create missing files from synced documents.
    ///
    /// This enables new peers to receive files by:
    /// 1. Processing the root index to find all `contents` references
    /// 2. Creating local files for paths that exist in sync but not locally
    /// 3. Recursively processing child index files
    pub fn discover_and_create_missing_files(&mut self) -> Result<usize, SyncError> {
        let mut files_created = 0;

        // Start with root index
        let root_index = PathBuf::from("index.md");
        files_created += self.process_index_and_children(&root_index)?;

        Ok(files_created)
    }

    /// Process an index file and its children, creating missing files.
    fn process_index_and_children(&mut self, index_path: &PathBuf) -> Result<usize, SyncError> {
        let mut files_created = 0;

        // Get the synced document for this index
        let Some(doc) = self.documents.get(index_path) else {
            return Ok(0);
        };

        let content = doc.get_content()?;
        let full_path = self.workspace_path.join(index_path);

        // Create the index file itself if it doesn't exist
        if !self.filesystem.exists(&full_path) {
            // Create parent directories
            if let Some(parent) = full_path.parent() {
                self.filesystem.create_dir_all(parent)?;
            }
            self.filesystem.write_file(&full_path, &content)?;
            files_created += 1;
        }

        // Parse contents from frontmatter
        let contents = Self::parse_contents_from_frontmatter(&content);

        // Get the directory containing this index
        let empty_path = PathBuf::new();
        let index_dir = index_path.parent().unwrap_or(&empty_path);

        for child_rel in contents {
            let child_path = if index_dir.as_os_str().is_empty() {
                PathBuf::from(&child_rel)
            } else {
                index_dir.join(&child_rel)
            };

            // Check if we have this document synced
            if let Some(child_doc) = self.documents.get(&child_path) {
                let child_full_path = self.workspace_path.join(&child_path);

                // Create if doesn't exist locally
                if !self.filesystem.exists(&child_full_path) {
                    // Create parent directories
                    if let Some(parent) = child_full_path.parent() {
                        self.filesystem.create_dir_all(parent)?;
                    }
                    let child_content = child_doc.get_content()?;
                    self.filesystem.write_file(&child_full_path, &child_content)?;
                    files_created += 1;
                }

                // If child is also an index, recurse
                let child_content = child_doc.get_content()?;
                if Self::is_index_file(&child_content) {
                    files_created += self.process_index_and_children(&child_path)?;
                }
            }
        }

        Ok(files_created)
    }

    /// Parse `contents` array from markdown frontmatter.
    fn parse_contents_from_frontmatter(content: &str) -> Vec<String> {
        // Extract frontmatter between --- delimiters
        let lines: Vec<&str> = content.lines().collect();
        if lines.is_empty() || lines[0].trim() != "---" {
            return vec![];
        }

        // Find end of frontmatter
        let mut end_idx = None;
        for (i, line) in lines.iter().enumerate().skip(1) {
            if line.trim() == "---" {
                end_idx = Some(i);
                break;
            }
        }

        let Some(end) = end_idx else {
            return vec![];
        };

        // Find contents: line and parse the array
        let mut in_contents = false;
        let mut contents = Vec::new();

        for line in &lines[1..end] {
            let trimmed = line.trim();
            
            if trimmed.starts_with("contents:") {
                in_contents = true;
                // Check for inline array: contents: [a, b, c]
                if let Some(array_start) = trimmed.find('[') {
                    if let Some(array_end) = trimmed.find(']') {
                        let items = &trimmed[array_start + 1..array_end];
                        for item in items.split(',') {
                            let item = item.trim().trim_matches('"').trim_matches('\'');
                            if !item.is_empty() {
                                contents.push(item.to_string());
                            }
                        }
                        in_contents = false;
                    }
                }
                continue;
            }

            if in_contents {
                // Check if this is a list item: - item
                if trimmed.starts_with('-') {
                    let item = trimmed[1..].trim().trim_matches('"').trim_matches('\'');
                    if !item.is_empty() {
                        contents.push(item.to_string());
                    }
                } else if !trimmed.is_empty() && !trimmed.starts_with(' ') && !trimmed.starts_with('\t') {
                    // New property, stop parsing contents
                    in_contents = false;
                }
            }
        }

        contents
    }

    /// Check if content represents an index file (has `contents` property).
    fn is_index_file(content: &str) -> bool {
        // Simple check: does frontmatter contain "contents:"
        let lines: Vec<&str> = content.lines().collect();
        if lines.is_empty() || lines[0].trim() != "---" {
            return false;
        }

        for line in lines.iter().skip(1) {
            let trimmed = line.trim();
            if trimmed == "---" {
                break;
            }
            if trimmed.starts_with("contents:") {
                return true;
            }
        }

        false
    }
}

/// Statistics from a sync round.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct SyncStats {
    /// Number of messages received from peers
    pub messages_received: usize,

    /// Number of messages sent to peers
    pub messages_sent: usize,

    /// Number of documents updated from peer changes
    pub documents_updated: usize,

    /// Number of files discovered and created from synced hierarchy
    pub files_discovered: usize,
}

#[cfg(test)]
#[cfg(feature = "live-sync")]
mod tests {
    use super::*;

    #[test]
    fn test_document_sync_create() {
        let content = "# Test Document\n\nThis is a test.".to_string();
        let doc_sync = DocumentSync::new(PathBuf::from("test.md"), content.clone()).unwrap();

        assert_eq!(doc_sync.path, PathBuf::from("test.md"));
        assert_eq!(doc_sync.get_content().unwrap(), content);
    }

    #[test]
    fn test_document_sync_update() {
        let content = "# Original".to_string();
        let mut doc_sync = DocumentSync::new(PathBuf::from("test.md"), content).unwrap();

        let new_content = "# Updated".to_string();
        doc_sync.update_content(new_content.clone()).unwrap();

        assert_eq!(doc_sync.get_content().unwrap(), new_content);
    }

    #[test]
    fn test_document_sync_messages() {
        let mut doc1 = DocumentSync::new(
            PathBuf::from("test.md"),
            "# Original".to_string(),
        )
        .unwrap();

        let mut doc2 = DocumentSync::new(
            PathBuf::from("test.md"),
            "# Original".to_string(),
        )
        .unwrap();

        // Update doc1
        doc1.update_content("# Updated by doc1".to_string()).unwrap();

        // Automerge sync requires bidirectional message exchange
        let peer1_id = "peer1".to_string();
        let peer2_id = "peer2".to_string();
        
        // Perform sync rounds until both docs converge
        for _ in 0..10 {
            // Doc1 generates sync message for doc2
            if let Some(msg) = doc1.generate_sync_message(&peer2_id).unwrap() {
                doc2.receive_sync_message(&peer1_id, &msg).unwrap();
            }
            
            // Doc2 generates sync message for doc1
            if let Some(msg) = doc2.generate_sync_message(&peer1_id).unwrap() {
                doc1.receive_sync_message(&peer2_id, &msg).unwrap();
            }
        }

        // Both documents should now have the same content
        assert_eq!(doc1.get_content().unwrap(), "# Updated by doc1");
        assert_eq!(doc2.get_content().unwrap(), "# Updated by doc1");
    }
}
