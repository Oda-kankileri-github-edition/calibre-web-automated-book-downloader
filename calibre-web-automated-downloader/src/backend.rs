use crate::book_manager;
use crate::config::CONFIG;
use crate::models::{BookInfo, QueueStatus, BOOK_QUEUE};
use lazy_static::lazy_static;
use log::{error, info};
use serde_json::json;
use std::collections::HashMap;
use std::{fs::File, io::Read, path::Path};
use tokio::sync::Mutex;

lazy_static! {
    static ref DOWNLOAD_MUTEX: Mutex<()> = Mutex::new(());
}

pub async fn search_books(query: &str) -> serde_json::Value {
    match book_manager::search_books(query, None).await {
        Ok(books) => {
            let book_list: Vec<_> = books
                .into_iter()
                .map(|b| serde_json::to_value(b).unwrap())
                .collect();
            json!(book_list)
        }
        Err(e) => {
            error!("Error searching books: {:?}", e);
            json!({ "error": "Failed to search books" })
        }
    }
}

pub async fn get_book_info(book_id: &str) -> serde_json::Value {
    match book_manager::get_book_info(book_id, None).await {
        Ok(book) => json!(serde_json::to_value(book).unwrap()),
        Err(e) => {
            error!("Error getting book info: {:?}", e);
            json!({ "error": "Failed to get book info" })
        }
    }
}

pub async fn queue_book(book_id: &str) -> bool {
    match book_manager::get_book_info(book_id, None).await {
        Ok(book_info) => {
            BOOK_QUEUE.add(book_id, book_info);
            info!("Book queued: {}", book_id);
            true
        }
        Err(e) => {
            error!("Error queueing book: {:?}", e);
            false
        }
    }
}

pub async fn queue_status() -> serde_json::Value {
    let statuses = BOOK_QUEUE.get_status();
    let response: HashMap<_, _> = statuses
        .into_iter()
        .map(|(status, books)| {
            (
                status.to_string(),
                books
                    .into_iter()
                    .map(|(id, book)| (id, serde_json::to_value(book).unwrap()))
                    .collect::<HashMap<_, _>>(),
            )
        })
        .collect();

    json!(response)
}

pub async fn get_book_data(book_id: &str) -> Option<(Vec<u8>, String)> {
    let data = BOOK_QUEUE.get_status();
    let book_info = data
        .get(&QueueStatus::Available)
        .and_then(|books| books.get(book_id).cloned());

    if let Some(book_info) = book_info {
        let file_path = CONFIG.ingest_dir.join(format!("{}.epub", book_id));

        let mut file = match File::open(&file_path) {
            Ok(f) => f,
            Err(e) => {
                error!("Error opening file {}: {:?}", file_path.display(), e);
                return None;
            }
        };

        let mut buffer = Vec::new();
        if let Err(e) = file.read_to_end(&mut buffer) {
            error!("Error reading file {}: {:?}", file_path.display(), e);
            return None;
        }

        return Some((buffer, book_info.title));
    }
    None
}

async fn download_book(book_id: &str) -> Result<(), anyhow::Error> {
    let book_info = BOOK_QUEUE
        .get_status()
        .get(&QueueStatus::Queued)
        .and_then(|books| books.get(book_id).cloned());

    if let Some(book_info) = book_info {
        let temp_path = CONFIG.tmp_dir.join(format!("{}.epub", book_id));

        book_manager::download_book(&book_info)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to download book: {:?}", e))?;

        if !process_book(&temp_path) {
            return Err(anyhow::anyhow!(
                "Failed to process book at {}",
                temp_path.display()
            ));
        }

        return Ok(());
    }

    Err(anyhow::anyhow!("Book not found in queue"))
}

fn process_book(path: &Path) -> bool {
    // Placeholder for book processing logic.
    // Currently, just checks if the file exists.
    if !path.exists() {
        error!("File not found: {}", path.display());
        return false;
    }
    info!("Successfully processed book: {}", path.display());
    true
}
