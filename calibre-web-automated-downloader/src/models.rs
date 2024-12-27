use once_cell::sync::Lazy;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::config::CONFIG;

/// An enum for possible book queue statuses.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum QueueStatus {
    Queued,
    Downloading,
    Available,
    Error,
    Done,
}

// (Optional) Provide a conversion to/from &'static str if desired.
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
#[derive(Clone, Debug)]
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
    /// Convenience constructor if desired
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

/// Thread-safe book queue manager
#[derive(Debug)]
pub struct BookQueue {
    queue: HashSet<String>,
    status: HashMap<String, QueueStatus>,
    book_data: HashMap<String, BookInfo>,
    status_timestamps: HashMap<String, Instant>,
    status_timeout: Duration,
}

impl BookQueue {
    /// Create a new empty queue with a default timeout (from CONFIG).
    pub fn new() -> Self {
        let timeout_secs = CONFIG.status_timeout;
        BookQueue {
            queue: HashSet::new(),
            status: HashMap::new(),
            book_data: HashMap::new(),
            status_timestamps: HashMap::new(),
            status_timeout: Duration::from_secs(timeout_secs),
        }
    }

    /// Internal helper to update the status + timestamp for a book ID
    fn update_status_internal(&mut self, book_id: &str, status: QueueStatus) {
        self.status.insert(book_id.to_string(), status);
        self.status_timestamps
            .insert(book_id.to_string(), Instant::now());
    }

    /// Add a book to the queue
    pub fn add(&mut self, book_id: &str, book_data: BookInfo) {
        self.queue.insert(book_id.to_string());
        self.book_data.insert(book_id.to_string(), book_data);
        self.update_status_internal(book_id, QueueStatus::Queued);
    }

    /// Get the next book in the queue (if any)
    pub fn get_next(&mut self) -> Option<String> {
        if let Some(book_id) = self.queue.iter().next().cloned() {
            self.queue.remove(&book_id);
            Some(book_id)
        } else {
            None
        }
    }

    /// Update status (public method)
    pub fn update_status(&mut self, book_id: &str, status: QueueStatus) {
        if self.status.contains_key(book_id) {
            self.update_status_internal(book_id, status);
        }
    }

    /// Return the current status of all books, grouped by QueueStatus.
    ///
    /// In Python: Dict[QueueStatus, Dict[str, BookInfo]]
    /// In Rust: HashMap<QueueStatus, HashMap<String, BookInfo>>
    pub fn get_status(&mut self) -> HashMap<QueueStatus, HashMap<String, BookInfo>> {
        // First refresh to remove stale/done items
        self.refresh();

        let mut result = HashMap::<QueueStatus, HashMap<String, BookInfo>>::new();

        // Initialize an empty HashMap for each status variant
        for status_variant in [
            QueueStatus::Queued,
            QueueStatus::Downloading,
            QueueStatus::Available,
            QueueStatus::Error,
            QueueStatus::Done,
        ] {
            result.insert(status_variant.clone(), HashMap::new());
        }

        for (book_id, status) in &self.status {
            if let Some(book_info) = self.book_data.get(book_id) {
                // Insert a clone so we don't move out of the original book_data
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
    /// - Removing stale entries that have exceeded the status_timeout
    ///   (but only if their status == DONE).
    pub fn refresh(&mut self) {
        let now = Instant::now();

        // We'll store which book IDs to update (mark as Done)
        let mut to_update = Vec::new();
        // We'll store which book IDs to remove entirely
        let mut to_remove = Vec::new();

        // First pass: just read immutable data and record changes to apply later
        for (book_id, status) in &self.status {
            // If status is AVAILABLE, check if the .epub file does NOT exist => mark as DONE
            if *status == QueueStatus::Available {
                let path = CONFIG.ingest_dir.join(format!("{}.epub", book_id));
                if !path.exists() {
                    to_update.push(book_id.clone()); // We'll update this after the loop
                }
            }

            // Check for stale entries if last update is too old
            if let Some(ts) = self.status_timestamps.get(book_id) {
                if now.duration_since(*ts) > self.status_timeout {
                    // Remove if it's DONE and timed out
                    if *status == QueueStatus::Done {
                        to_remove.push(book_id.clone());
                    }
                }
            }
        }

        // Second pass: apply updates (which requires mutating self).
        // Now the immutable borrow from the for-loop is no longer in use.
        for book_id in to_update {
            self.update_status_internal(&book_id, QueueStatus::Done);
        }

        // Finally remove stale entries
        for book_id in to_remove {
            self.status.remove(&book_id);
            self.status_timestamps.remove(&book_id);
            self.book_data.remove(&book_id);
        }
    }

    /// Change the status timeout in hours
    pub fn set_status_timeout(&mut self, hours: u64) {
        self.status_timeout = Duration::from_secs(hours * 3600);
    }
}

// Global instance of BookQueue, protected by a Mutex for thread safety.
// Access with `BOOK_QUEUE.lock().unwrap()` to get a mutable reference.
pub static BOOK_QUEUE: Lazy<Mutex<BookQueue>> = Lazy::new(|| {
    let queue = BookQueue::new();
    Mutex::new(queue)
});
