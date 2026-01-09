//! Global filesystem state management.

use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

use diaryx_core::fs::{InMemoryFileSystem, SyncToAsyncFs};

thread_local! {
    static FILESYSTEM: RefCell<InMemoryFileSystem> = RefCell::new(InMemoryFileSystem::new());
}

/// Execute a closure with read access to the global filesystem.
pub fn with_fs<F, R>(f: F) -> R
where
    F: FnOnce(&InMemoryFileSystem) -> R,
{
    FILESYSTEM.with(|fs| f(&fs.borrow()))
}

/// Execute a closure with access to the global filesystem.
///
/// Note: Uses immutable borrow because `InMemoryFileSystem` uses internal
/// mutability (`RefCell<HashMap>`). The `FileSystem` trait is implemented
/// for `&InMemoryFileSystem`, not `&mut InMemoryFileSystem`.
pub fn with_fs_mut<F, R>(f: F) -> R
where
    F: FnOnce(&InMemoryFileSystem) -> R,
{
    FILESYSTEM.with(|fs| f(&fs.borrow()))
}

/// Execute a closure with an async filesystem wrapper.
///
/// This wraps the sync InMemoryFileSystem with SyncToAsyncFs for use
/// with async-first core modules (Workspace, Validator, Searcher, etc.).
pub fn with_async_fs<F, R>(f: F) -> R
where
    F: FnOnce(SyncToAsyncFs<&InMemoryFileSystem>) -> R,
{
    FILESYSTEM.with(|fs| {
        let borrowed = fs.borrow();
        let async_fs = SyncToAsyncFs::new(&*borrowed);
        f(async_fs)
    })
}

/// Simple blocking executor for running async futures in WASM.
///
/// Since InMemoryFileSystem is synchronous, futures from SyncToAsyncFs
/// complete immediately without yielding, making this safe to use.
pub fn block_on<F: Future>(f: F) -> F::Output {
    // Create a no-op waker
    const VTABLE: RawWakerVTable = RawWakerVTable::new(
        |_| RawWaker::new(std::ptr::null(), &VTABLE), // clone
        |_| {},                                       // wake
        |_| {},                                       // wake_by_ref
        |_| {},                                       // drop
    );

    let raw_waker = RawWaker::new(std::ptr::null(), &VTABLE);
    let waker = unsafe { Waker::from_raw(raw_waker) };
    let mut cx = Context::from_waker(&waker);

    let mut pinned = std::pin::pin!(f);
    loop {
        match pinned.as_mut().poll(&mut cx) {
            Poll::Ready(output) => return output,
            Poll::Pending => {
                // For sync-wrapped futures, this should never happen
                // But we handle it anyway by spinning
                std::hint::spin_loop();
            }
        }
    }
}

/// Replace the entire filesystem with a new one.
///
/// Use this for operations that need to replace the whole filesystem
/// (e.g., loading from backup, initial load).
pub fn replace_fs(new_fs: InMemoryFileSystem) {
    FILESYSTEM.with(|fs| *fs.borrow_mut() = new_fs);
}

/// Reset the filesystem to a fresh state (for testing).
#[cfg(test)]
pub fn reset_filesystem() {
    FILESYSTEM.with(|fs| *fs.borrow_mut() = InMemoryFileSystem::new());
}
