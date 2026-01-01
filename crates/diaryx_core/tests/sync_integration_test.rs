//! Integration tests for live sync functionality

#[cfg(test)]
#[cfg(feature = "live-sync")]
mod sync_integration_tests {
    use diaryx_core::fs::{FileSystem, RealFileSystem};
    use diaryx_core::sync_crdt::{DocumentSync, LiveSyncProvider, PeerId, SyncError};
    use std::collections::VecDeque;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    /// Mock sync provider for testing (simulates network)
    struct MockSyncProvider {
        connected: bool,
        peer_id: String,
        outgoing: Arc<Mutex<VecDeque<Vec<u8>>>>,
        incoming: Arc<Mutex<VecDeque<Vec<u8>>>>,
    }

    impl MockSyncProvider {
        fn new(peer_id: String) -> Self {
            Self {
                connected: false,
                peer_id,
                outgoing: Arc::new(Mutex::new(VecDeque::new())),
                incoming: Arc::new(Mutex::new(VecDeque::new())),
            }
        }

        /// Connect two mock providers so they can exchange messages
        fn connect_peers(provider1: &Self, provider2: &Self) {
            // Provider1's outgoing becomes Provider2's incoming and vice versa
            let out1 = Arc::clone(&provider1.outgoing);
            let in2 = Arc::clone(&provider2.incoming);
            let out2 = Arc::clone(&provider2.outgoing);
            let in1 = Arc::clone(&provider1.incoming);

            // In a real test, we'd need to periodically move messages
            // For now, we'll do it manually in the test
        }
    }

    impl LiveSyncProvider for MockSyncProvider {
        fn connect(&mut self) -> Result<(), SyncError> {
            self.connected = true;
            Ok(())
        }

        fn disconnect(&mut self) -> Result<(), SyncError> {
            self.connected = false;
            Ok(())
        }

        fn send_sync_message(&self, message: Vec<u8>) -> Result<(), SyncError> {
            self.outgoing.lock().unwrap().push_back(message);
            Ok(())
        }

        fn receive_sync_messages(&self) -> Result<Vec<Vec<u8>>, SyncError> {
            let mut incoming = self.incoming.lock().unwrap();
            let messages: Vec<_> = incoming.drain(..).collect();
            Ok(messages)
        }

        fn is_connected(&self) -> bool {
            self.connected
        }

        fn peer_id(&self) -> Option<PeerId> {
            if self.connected {
                Some(self.peer_id.clone())
            } else {
                None
            }
        }
    }

    #[test]
    fn test_two_documents_sync() {
        // Create two documents with same initial content
        let mut doc1 = DocumentSync::new(
            PathBuf::from("test.md"),
            "# Original Content".to_string(),
        )
        .unwrap();

        let mut doc2 = DocumentSync::new(
            PathBuf::from("test.md"),
            "# Original Content".to_string(),
        )
        .unwrap();

        // Update doc1
        doc1.update_content("# Updated by Doc1".to_string())
            .unwrap();

        let peer1_id = "peer1".to_string();
        let peer2_id = "peer2".to_string();

        // Perform bidirectional sync
        for _ in 0..10 {
            // Doc1 -> Doc2
            if let Some(msg) = doc1.generate_sync_message(&peer2_id).unwrap() {
                doc2.receive_sync_message(&peer1_id, &msg).unwrap();
            }

            // Doc2 -> Doc1
            if let Some(msg) = doc2.generate_sync_message(&peer1_id).unwrap() {
                doc1.receive_sync_message(&peer2_id, &msg).unwrap();
            }
        }

        // Both should have the same content now
        assert_eq!(doc1.get_content().unwrap(), "# Updated by Doc1");
        assert_eq!(doc2.get_content().unwrap(), "# Updated by Doc1");
    }

    #[test]
    fn test_concurrent_edits_converge() {
        // Create two documents
        let mut doc1 = DocumentSync::new(
            PathBuf::from("test.md"),
            "# Start".to_string(),
        )
        .unwrap();

        let mut doc2 = DocumentSync::new(
            PathBuf::from("test.md"),
            "# Start".to_string(),
        )
        .unwrap();

        // Both make concurrent edits
        doc1.update_content("# Updated by A".to_string()).unwrap();
        doc2.update_content("# Updated by B".to_string()).unwrap();

        let peer1_id = "peer1".to_string();
        let peer2_id = "peer2".to_string();

        // Sync them
        for _ in 0..10 {
            if let Some(msg) = doc1.generate_sync_message(&peer2_id).unwrap() {
                doc2.receive_sync_message(&peer1_id, &msg).unwrap();
            }

            if let Some(msg) = doc2.generate_sync_message(&peer1_id).unwrap() {
                doc1.receive_sync_message(&peer2_id, &msg).unwrap();
            }
        }

        // They should converge to the same content
        // (automerge will resolve the conflict deterministically)
        let content1 = doc1.get_content().unwrap();
        let content2 = doc2.get_content().unwrap();
        assert_eq!(content1, content2);
        println!("Converged to: {}", content1);
    }
}
