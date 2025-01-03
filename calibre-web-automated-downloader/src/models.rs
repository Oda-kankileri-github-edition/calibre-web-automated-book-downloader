use once_cell::sync::Lazy;
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

// Bring the macros and other important things into scope.
use proptest::prelude::*;

use crate::config::CONFIG;

/// An enum for possible book queue statuses.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Arbitrary)]
pub enum QueueStatus {
    Queued,
    Downloading,
    Available,
    Error,
    Done,
}

impl ToString for QueueStatus {
    fn to_string(&self) -> String {
        match self {
            QueueStatus::Queued => "queued",
            QueueStatus::Downloading => "downloading",
            QueueStatus::Available => "available",
            QueueStatus::Error => "error",
            QueueStatus::Done => "done",
        }
        .to_string()
    }
}

/// Data structure representing book information.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BookInfo {
    pub id: String,
    pub title: String,
    pub preview: Option<String>,
    pub author: Option<String>,
    pub publisher: Option<String>,
    pub year: Option<String>,
    pub language: Option<String>,
    pub format: Option<String>,
    pub size: Option<String>,

    /// e.g. info: { "isbn": ["1234", "9876"], "tags": ["something"] }
    pub info: Option<HashMap<String, Vec<String>>>,

    /// e.g. a list of direct download URLs
    pub download_urls: Vec<String>,
}

impl BookInfo {
    pub fn new(id: &str, title: &str) -> Self {
        Self {
            id: id.to_owned(),
            title: title.to_owned(),
            preview: None,
            author: None,
            publisher: None,
            year: None,
            language: None,
            format: None,
            size: None,
            info: None,
            download_urls: vec![],
        }
    }
}

/// The **data** holding all the shared state of the queue.
/// We do not expose this directly because callers
/// should only interact with the public methods on `BookQueue`.
#[derive(Debug)]
struct BookQueueData {
    // This implementation is horrible, using the same book_id as the key in multiple maps.
    queue: HashSet<String>,
    status: HashMap<String, QueueStatus>,
    book_data: HashMap<String, BookInfo>,
    status_timestamps: HashMap<String, Instant>,
    status_timeout: Duration,
}

/// Thread-safe book queue manager.
/// All concurrency is handled by the internal `Mutex`.
#[derive(Debug)]
pub struct BookQueue {
    data: Mutex<BookQueueData>,
}

impl BookQueue {
    /// Create a new empty queue with a default timeout (from CONFIG).
    pub fn new() -> Self {
        let timeout_secs = CONFIG.status_timeout;
        let data = BookQueueData {
            queue: HashSet::new(),
            status: HashMap::new(),
            book_data: HashMap::new(),
            status_timestamps: HashMap::new(),
            status_timeout: Duration::from_secs(timeout_secs),
        };
        BookQueue {
            data: Mutex::new(data),
        }
    }

    /// Internal helper to update the status + timestamp for a book ID.
    fn update_status_internal(data: &mut BookQueueData, book_id: &str, status: QueueStatus) {
        data.status.insert(book_id.to_string(), status);
        data.status_timestamps
            .insert(book_id.to_string(), Instant::now());
    }

    /// Add a book to the queue.
    pub fn add(&self, book_id: &str, book_data: BookInfo) {
        let mut data = self.data.lock().unwrap();
        data.queue.insert(book_id.to_string());
        data.book_data.insert(book_id.to_string(), book_data);
        Self::update_status_internal(&mut data, book_id, QueueStatus::Queued);
    }

    /// Get the next book in the queue (if any).
    pub fn get_next(&self) -> Option<String> {
        let mut data = self.data.lock().unwrap();
        if let Some(book_id) = data.queue.iter().next().cloned() {
            data.queue.remove(&book_id);
            Some(book_id)
        } else {
            None
        }
    }

    /// Update the status of an existing book.
    pub fn update_status(&self, book_id: &str, status: QueueStatus) {
        let mut data = self.data.lock().unwrap();
        log::debug!("Checking status of {} to {:?}", book_id, status);
        if data.status.contains_key(book_id) {
            log::debug!("Updating status of {} to {:?}", book_id, status);
            Self::update_status_internal(&mut data, book_id, status);
        }
    }

    /// Return the current status of all books, grouped by QueueStatus.
    pub fn get_status(&self) -> HashMap<QueueStatus, HashMap<String, BookInfo>> {
        let mut data = self.data.lock().unwrap();

        // First refresh to remove stale/done items
        Self::refresh_internal(&mut data);

        // Build a HashMap<QueueStatus, HashMap<String, BookInfo>>
        let mut result = HashMap::<QueueStatus, HashMap<String, BookInfo>>::new();

        // Pre-populate each status variant with an empty map
        result = HashMap::from([
            (QueueStatus::Queued, HashMap::new()),
            (QueueStatus::Downloading, HashMap::new()),
            (QueueStatus::Available, HashMap::new()),
            (QueueStatus::Error, HashMap::new()),
            (QueueStatus::Done, HashMap::new()),
        ]);
        // Fill each map with cloned BookInfo
        for (book_id, status) in &data.status {
            if let Some(book_info) = data.book_data.get(book_id) {
                result
                    .get_mut(status)
                    .unwrap()
                    .insert(book_id.clone(), book_info.clone());
            }
        }

        result
    }

    /// Refresh the queue by:
    /// - Checking if "AVAILABLE" books have an .epub file; if not, mark them DONE.
    /// - Removing stale entries that have exceeded the status_timeout (but only if they are DONE).
    fn refresh_internal(data: &mut BookQueueData) {
        let now = Instant::now();
        let mut to_update = Vec::new();
        let mut to_remove = Vec::new();

        log::debug!("Refreshing queue...");

        // First pass: record changes
        for (book_id, status) in &data.status {
            log::debug!("Checking status of {}: {:?}", book_id, status);
            if *status == QueueStatus::Available {
                let path = CONFIG.ingest_dir.join(format!("{}.epub", book_id));
                if !path.exists() {
                    to_update.push(book_id.clone());
                }
            }

            if let Some(ts) = data.status_timestamps.get(book_id) {
                log::debug!(
                    "Checking timestamp of {}: {:?}, {:?}",
                    book_id,
                    now.duration_since(*ts),
                    data.status_timeout
                );
                if now.duration_since(*ts) > data.status_timeout {
                    log::debug!("Stale entry: {}", book_id);
                    if *status == QueueStatus::Done {
                        log::debug!("Marking stale entry as DONE: {}", book_id);
                        to_remove.push(book_id.clone());
                    }
                }
            }
        }

        // Second pass: apply updates
        for book_id in to_update {
            Self::update_status_internal(data, &book_id, QueueStatus::Done);
        }

        // Remove stale entries
        for book_id in to_remove {
            log::debug!("Removing stale entry: {}", book_id);
            data.status.remove(&book_id);
            data.status_timestamps.remove(&book_id);
            data.book_data.remove(&book_id);
            data.queue.remove(&book_id);
        }
    }

    /// Public refresh method: lock and delegate.
    pub fn refresh(&self) {
        let mut data = self.data.lock().unwrap();
        Self::refresh_internal(&mut data);
    }

    /// Change the status timeout in hours.
    pub fn set_status_timeout(&self, hours: u64) {
        let mut data = self.data.lock().unwrap();
        data.status_timeout = Duration::from_secs(hours * 3600);
    }
}

/// A global, lazily initialized instance of BookQueue (thread-safe by design).
pub static BOOK_QUEUE: Lazy<BookQueue> = Lazy::new(BookQueue::new);

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn test_book_queue_status_to_string() {
        assert_eq!(QueueStatus::Queued.to_string(), "queued");
        assert_eq!(QueueStatus::Downloading.to_string(), "downloading");
        assert_eq!(QueueStatus::Available.to_string(), "available");
        assert_eq!(QueueStatus::Error.to_string(), "error");
        assert_eq!(QueueStatus::Done.to_string(), "done");
    }

    #[test]
    fn test_book_queue() {
        let queue = BookQueue::new();
        let book_id = "ABCD";
        let status = QueueStatus::Done;
        // Test enqueue
        queue.add(book_id, BookInfo::new(book_id, "Title"));
        // Test update
        queue.update_status(book_id, status);
        // Change the clock to simulate a timeout
        queue.set_status_timeout(0);
        queue.refresh();
        // Check if the queue is empty
        assert_eq!(queue.get_next(), None);
    }

    #[test]
    fn test_book_queue_status() {
        let queue = BookQueue::new();
        let book_id = "ABCD";
        // Test enqueue
        queue.add(book_id, BookInfo::new(book_id, "Title"));
        // Test status update
        queue.update_status(book_id, QueueStatus::Downloading);
        queue.refresh();
        // Check if the status is updated
        assert!(queue
            .get_status()
            .get(&QueueStatus::Downloading)
            .is_some_and(|v| v.len() == 1));
    }

    #[test]
    // Test updating a non-existent book status
    fn test_book_queue_status_non_existent() {
        let queue = BookQueue::new();
        let book_id = "ABCD";
        queue.update_status(book_id, QueueStatus::Downloading);
        queue.refresh();
        assert!(queue
            .get_status()
            .get(&QueueStatus::Downloading)
            .is_some_and(|v| v.len() == 0));
    }

    // Test thread safety
    #[test]
    fn test_book_queue_threadsafe() {
        let queue = Arc::new(BookQueue::new());
        let book_id = "ABCD";
        let handles = (0..100)
            .map(|i| {
                let queue_ref = Arc::clone(&queue);
                std::thread::spawn(move || {
                    let b = format!("{}-{}", book_id, i);
                    queue_ref.add(&b, BookInfo::new(book_id, "Title"));
                    queue_ref.update_status(&b, QueueStatus::Downloading);
                })
            })
            .collect::<Vec<_>>();
        for handle in handles {
            handle.join().unwrap();
        }
        queue.refresh();
        println!("{:?}", queue.get_status().len());
        assert!(queue
            .get_status()
            .get(&QueueStatus::Downloading)
            .is_some_and(|v| v.len() == 100));
    }

    proptest! {
        #[test]
        fn test_book_queue_proptest(status in any::<QueueStatus>()) {
            let queue = BookQueue::new();
            let book_id = "ABCD";
            queue.add(&book_id, BookInfo::new(&book_id, "Title"));
            queue.update_status(&book_id, status.clone());
            if status == QueueStatus::Available {
                prop_assert!(queue.get_status().get(&QueueStatus::Done).is_some_and(|v| v.len() == 1));
            } else {
                prop_assert!(queue.get_status().get(&status).is_some_and(|v| v.len() == 1));
            }
        }

        #[test]
        fn test_book_queue_proptest_refresh(status in any::<QueueStatus>()) {
            let queue = BookQueue::new();
            let book_id = "ABCD";
            queue.add(&book_id, BookInfo::new(&book_id, "Title"));
            queue.update_status(&book_id, status.clone());
            queue.refresh();
            if status == QueueStatus::Available {
                prop_assert!(queue.get_status().get(&QueueStatus::Done).is_some_and(|v| v.len() == 1));
            } else {
                prop_assert!(queue.get_status().get(&status).is_some_and(|v| v.len() == 1));
            }
        }

        #[test]
        fn test_book_queue_proptest_threadsafe(count in 0usize..100) {
            let queue = Arc::new(BookQueue::new());
            let book_id = "ABCD";
            let handles = (0..count).map(|i| {
                let queue_ref = Arc::clone(&queue);
                std::thread::spawn(move || {
                    let b =  format!("{}-{}", book_id, i);
                    queue_ref.add(&b, BookInfo::new(book_id, "Title"));
                    queue_ref.update_status(&b, QueueStatus::Downloading);
                })
            }).collect::<Vec<_>>();
            for handle in handles {
                handle.join().unwrap();
            }
            queue.refresh();
            prop_assert!(queue.get_status().get(&QueueStatus::Downloading).is_some_and(|v| v.len() == count));
        }
    }
}
