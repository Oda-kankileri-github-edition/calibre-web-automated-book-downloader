use crate::config::CONFIG;
use crate::models::{BookInfo, QueueStatus, BOOK_QUEUE};
use crate::network;
use anyhow::{anyhow, Result};
use scraper::{Html, Selector};
use std::collections::HashMap;
use urlencoding::encode;

/// Search for books based on a query.
pub async fn search_books(query: &str, base_url: Option<&str>) -> Result<Vec<BookInfo>> {
    let base_url = base_url.unwrap_or(&CONFIG.aa_base_url);
    let query_url = format!(
        "{}/search?index=&page=1&display=table&acc=aa_download&acc=external_download&sort=&ext={}&lang={}&q={}",
        base_url,
        CONFIG.supported_formats.join("&ext="),
        CONFIG.book_language.join("&lang="),
        encode(query)
    );

    let html = network::html_get_page(query_url).await?;
    if html.contains("No files found.") {
        return Err(anyhow!("No books found for query: {}", query));
    }

    parse_search_results(&html)
}

/// Parse search results into a vector of `BookInfo`.
fn parse_search_results(html: &str) -> Result<Vec<BookInfo>> {
    let document = Html::parse_document(html);
    let table_selector = Selector::parse("table").unwrap();
    let row_selector = Selector::parse("tr").unwrap();

    let mut books = Vec::new();
    if let Some(table) = document.select(&table_selector).next() {
        for row in table.select(&row_selector) {
            if let Some(book) = parse_search_result_row(&row)? {
                books.push(book);
            }
        }
    }

    Ok(books)
}

fn parse_search_result_row(row: &scraper::ElementRef) -> Result<Option<BookInfo>> {
    let cell_selector = Selector::parse("td").unwrap();
    let link_selector = Selector::parse("a").unwrap();
    let img_selector = Selector::parse("img").unwrap();
    let cells: Vec<_> = row.select(&cell_selector).collect();

    // Return None if the row doesn't have enough cells
    if cells.len() < 11 {
        return Ok(None);
    }

    // Extract `id` from the href attribute of the `<a>` tag in the first cell
    let id = match cells[0]
        .select(&link_selector)
        .next()
        .and_then(|link| link.value().attr("href"))
        .and_then(|href| href.split('/').last())
    {
        Some(id) => id.to_string(),
        None => return Ok(None),
    };

    // Extract `preview` from the `<img>` tag in the first cell
    let preview = cells[0]
        .select(&img_selector)
        .next()
        .and_then(|img| img.value().attr("src"))
        .map(|s| s.to_string());

    // Create the BookInfo object
    let book_info = BookInfo {
        id,
        preview,
        title: cells[1].text().next().unwrap_or("").to_string(),
        author: Some(cells[2].text().next().unwrap_or("").to_string()),
        publisher: Some(cells[3].text().next().unwrap_or("").to_string()),
        year: Some(cells[4].text().next().unwrap_or("").to_string()),
        language: Some(cells[7].text().next().unwrap_or("").to_string()),
        format: Some(cells[9].text().next().unwrap_or("").to_string()),
        size: Some(cells[10].text().next().unwrap_or("").to_string()),
        info: None,
        download_urls: vec![],
    };

    Ok(Some(book_info))
}

/// Fetch detailed information for a specific book.
pub async fn get_book_info(book_id: &str, base_url: Option<&str>) -> Result<BookInfo> {
    let base_url = base_url.unwrap_or(&CONFIG.aa_base_url);

    let url = format!("{}/md5/{}", base_url, book_id);
    let html = network::html_get_page(url).await?;
    parse_book_info_page(&html, book_id)
}

/// Parse detailed book information from an HTML page.
fn parse_book_info_page(html: &str, book_id: &str) -> Result<BookInfo> {
    let document = Html::parse_document(html);
    let main_selector =
        Selector::parse("body > main > div").map_err(|e| anyhow!("Invalid selector: {}", e))?;

    let data = document
        .select(&main_selector)
        .next()
        .ok_or_else(|| anyhow!("Failed to find main container for book ID: {}", book_id))?;

    let preview = data
        .select(&Selector::parse("div img").unwrap())
        .next()
        .and_then(|img| img.value().attr("src"))
        .map(|s| s.to_string());

    let divs: Vec<_> = data.select(&Selector::parse("div").unwrap()).collect();
    let start_div_id = divs
        .iter()
        .position(|div| div.text().any(|text| text.contains("🔍")))
        .unwrap_or(3);

    let format_div = divs
        .get(start_div_id - 1)
        .map(|div| div.text().collect::<Vec<_>>().concat())
        .unwrap_or_default();
    let format = format_div
        .split('.')
        .nth(1)
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim().to_lowercase());
    let size = format_div
        .split(',')
        .find(|token| {
            token
                .trim()
                .chars()
                .next()
                .map_or(false, |c| c.is_numeric())
        })
        .map(|s| s.trim().to_string());

    let mut urls = vec![];
    for anchor in document.select(&Selector::parse("a").unwrap()) {
        if let Some(href) = anchor.value().attr("href") {
            urls.push(href.to_string());
        }
    }

    let mut book_info = BookInfo {
        id: book_id.to_string(),
        title: divs
            .get(start_div_id)
            .map(|div| div.text().collect::<Vec<_>>().concat())
            .unwrap_or_default(),
        publisher: divs
            .get(start_div_id + 1)
            .map(|div| div.text().collect::<Vec<_>>().concat()),
        author: divs
            .get(start_div_id + 2)
            .map(|div| div.text().collect::<Vec<_>>().concat()),
        format,
        size,
        language: None,
        year: None,
        preview: preview,
        download_urls: urls,
        info: Some(HashMap::new()),
    };

    book_info.info = Some(extract_book_metadata(&divs[start_div_id + 3..]));

    if let Some(language) = book_info
        .info
        .as_ref()
        .and_then(|info| info.get("Language").and_then(|v| v.get(0)))
    {
        book_info.language = Some(language.clone());
    }
    if let Some(year) = book_info
        .info
        .as_ref()
        .and_then(|info| info.get("Year").and_then(|v| v.get(0)))
    {
        book_info.year = Some(year.clone());
    }

    Ok(book_info)
}

fn extract_book_metadata(metadata_divs: &[scraper::ElementRef]) -> HashMap<String, Vec<String>> {
    let mut info = HashMap::new();

    for div in metadata_divs {
        let sub_data: Vec<_> = div.select(&Selector::parse("div").unwrap()).collect();
        for i in (0..sub_data.len()).step_by(2) {
            if let (Some(key), Some(value)) = (
                sub_data[i].text().next().map(|s| s.trim().to_string()),
                sub_data
                    .get(i + 1)
                    .and_then(|v| v.text().next())
                    .map(|s| s.trim().to_string()),
            ) {
                info.entry(key).or_insert_with(Vec::new).push(value);
            }
        }

        if let Some(spans) = div.select(&Selector::parse("span").unwrap()).next() {
            for span in spans.select(&Selector::parse("span").unwrap()).step_by(2) {
                let key = span.text().next().unwrap_or_default().trim().to_string();
                let value = span.text().nth(1).unwrap_or_default().trim().to_string();
                info.entry(key).or_insert_with(Vec::new).push(value);
            }
        }
    }

    info
}

/// Download a book based on its `BookInfo`.
pub async fn download_book(book_info: &BookInfo) -> Result<()> {
    for url in &book_info.download_urls {
        if let Ok(data) = network::download_url(url).await {
            let path = CONFIG.tmp_dir.join(format!(
                "{}.{}",
                book_info.id,
                book_info.format.clone().unwrap_or_default()
            ));
            tokio::fs::write(path, data).await?;
            return Ok(());
        }
    }

    Err(anyhow!("Failed to download book"))
}

/// Queue a book for downloading.
pub fn queue_book(book_id: &str, book_info: BookInfo) {
    BOOK_QUEUE.add(book_id, book_info);
}

/// Get the current status of the book queue.
pub fn get_queue_status() -> HashMap<QueueStatus, HashMap<String, BookInfo>> {
    BOOK_QUEUE.get_status()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{fs, test};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // tests for search_books and its helpers
    #[test]
    async fn test_parse_search_result_row_complete_row() {
        let html = r#"
        <table>
            <tr>
                <td><a href="/book1"><img src="preview1.jpg" /></a></td>
                <td>Book Title</td>
                <td>Author</td>
                <td>Publisher</td>
                <td>2021</td>
                <td></td><td></td><td>English</td>
                <td></td><td>epub</td>
                <td>1.5MB</td>
            </tr>
        </table>
        "#;
        let document = Html::parse_document(html);
        let row_selector = Selector::parse("tr").unwrap();
        let row = document.select(&row_selector).next().unwrap();

        let result = parse_search_result_row(&row).unwrap();
        let book = result.unwrap();

        assert_eq!(book.id, "book1");
        assert_eq!(book.preview.as_deref(), Some("preview1.jpg"));
        assert_eq!(book.title, "Book Title");
        assert_eq!(book.author.as_deref(), Some("Author"));
        assert_eq!(book.publisher.as_deref(), Some("Publisher"));
        assert_eq!(book.year.as_deref(), Some("2021"));
        assert_eq!(book.language.as_deref(), Some("English"));
        assert_eq!(book.format.as_deref(), Some("epub"));
        assert_eq!(book.size.as_deref(), Some("1.5MB"));
    }

    #[test]
    async fn test_parse_search_results_empty_table() {
        let html = "<table></table>";

        let books = parse_search_results(html).unwrap();
        assert!(books.is_empty());
    }

    #[test]
    async fn test_parse_search_results_with_data() {
        let html = r#"
        <table>
            <tr>
                <td><a href="/book1"></a></td>
                <td>Book Title</td>
                <td>Author</td>
                <td>Publisher</td>
                <td>2021</td>
                <td></td><td></td><td>English</td>
                <td></td><td>epub</td>
                <td>1.5MB</td>
            </tr>
        </table>
        "#;

        let books = parse_search_results(html).unwrap();

        assert_eq!(books.len(), 1);
        let book = &books[0];
        assert_eq!(book.id, "book1");
        assert_eq!(book.title, "Book Title");
        assert_eq!(book.author.as_deref(), Some("Author"));
        assert_eq!(book.publisher.as_deref(), Some("Publisher"));
        assert_eq!(book.year.as_deref(), Some("2021"));
        assert_eq!(book.language.as_deref(), Some("English"));
        assert_eq!(book.format.as_deref(), Some("epub"));
        assert_eq!(book.size.as_deref(), Some("1.5MB"));
    }

    #[test]
    async fn test_parse_search_result_row_incomplete_row() {
        let html = r#"
        <table>
            <tr><td>Incomplete Row</td></tr>
        </table>
        "#;
        let document = Html::parse_document(html);
        let row_selector = Selector::parse("tr").unwrap();
        let row = document.select(&row_selector).next().unwrap();

        let result = parse_search_result_row(&row).unwrap();

        // Verify that None is returned for incomplete rows
        assert!(result.is_none());
    }

    #[test]
    async fn test_search_books_success() {
        // Start a mock server
        let mock_server = MockServer::start().await;

        // Define an example HTML response for search results
        let example_html = r#"
    <table>
        <tr>
            <td><a href="/book1"><img src="preview1.jpg" /></a></td>
            <td>Book Title 1</td>
            <td>Author 1</td>
            <td>Publisher 1</td>
            <td>2021</td>
            <td></td><td></td><td>English</td>
            <td></td><td>epub</td>
            <td>1.5MB</td>
        </tr>
        <tr>
            <td><a href="/book2"><img src="preview2.jpg" /></a></td>
            <td>Book Title 2</td>
            <td>Author 2</td>
            <td>Publisher 2</td>
            <td>2022</td>
            <td></td><td></td><td>German</td>
            <td></td><td>pdf</td>
            <td>2.3MB</td>
        </tr>
    </table>
    "#;

        // Mock the response for the search endpoint
        Mock::given(method("GET"))
            .and(path("/search"))
            .respond_with(ResponseTemplate::new(200).set_body_string(example_html))
            .mount(&mock_server)
            .await;

        // Call the search_books function with the mock server's URL
        let query = "example query";
        let books = search_books(query, Some(&mock_server.uri())).await.unwrap();

        // Verify results
        assert_eq!(books.len(), 2);

        // Verify the first book
        let book1 = &books[0];
        assert_eq!(book1.id, "book1");
        assert_eq!(book1.preview.as_deref(), Some("preview1.jpg"));
        assert_eq!(book1.title, "Book Title 1");
        assert_eq!(book1.author.as_deref(), Some("Author 1"));
        assert_eq!(book1.publisher.as_deref(), Some("Publisher 1"));
        assert_eq!(book1.year.as_deref(), Some("2021"));
        assert_eq!(book1.language.as_deref(), Some("English"));
        assert_eq!(book1.format.as_deref(), Some("epub"));
        assert_eq!(book1.size.as_deref(), Some("1.5MB"));

        // Verify the second book
        let book2 = &books[1];
        assert_eq!(book2.id, "book2");
        assert_eq!(book2.preview.as_deref(), Some("preview2.jpg"));
        assert_eq!(book2.title, "Book Title 2");
        assert_eq!(book2.author.as_deref(), Some("Author 2"));
        assert_eq!(book2.publisher.as_deref(), Some("Publisher 2"));
        assert_eq!(book2.year.as_deref(), Some("2022"));
        assert_eq!(book2.language.as_deref(), Some("German"));
        assert_eq!(book2.format.as_deref(), Some("pdf"));
        assert_eq!(book2.size.as_deref(), Some("2.3MB"));
    }

    #[test]
    async fn test_search_books_success_danmachi() {
        // Start a mock server
        let mock_server = MockServer::start().await;

        // Load HTML content from file
        let html_content = fs::read_to_string("./test_data/danmachi.html")
            .await
            .expect("Failed to read danmachi.html");

        // Mock the response for the search endpoint
        Mock::given(method("GET"))
            .and(path("/search"))
            .respond_with(ResponseTemplate::new(200).set_body_string(html_content))
            .mount(&mock_server)
            .await;

        // Call the search_books function with the mock server's URL
        let query = "ダンジョンに出会いを求めるのは間違っているだろうか";
        let books = search_books(query, Some(&mock_server.uri())).await.unwrap();

        // Verify results
        assert_eq!(books.len(), 100);

        // Verify the first book
        let book1 = &books[0];
        assert_eq!(book1.id, "9320e010092ad5cde279f733bdda3a2f");
        assert_eq!(book1.preview.as_deref(), Some("https://s3proxy.cdn-zlib.sk//covers299/collections/userbooks/96f72585a12a73923dbac5e0769e41c6a98314c6f893599cc6bb0314c0f3b48e.jpg"));
        assert_eq!(book1.title, "Is It Wrong to Try to Pick Up Girls in a Dungeon?, Vol. 18");
        assert_eq!(book1.author.as_deref(), Some("Fujino Omori and Suzuhito Yasuda"));
        assert_eq!(book1.publisher.as_deref(), Some("Yen On, Is It Wrong to Try to Pick Up Girls in a Dungeon?, 18, 2023"));
        assert_eq!(book1.year.as_deref(), Some("2023"));
        assert_eq!(book1.language.as_deref(), Some("en"));
        assert_eq!(book1.format.as_deref(), Some("epub"));
        assert_eq!(book1.size.as_deref(), Some("10.2MB"));
    }

    // tests for get_book_info and its helpers
    #[test]
    async fn test_get_book_info() {
        let book_id = "10bc7868c3d8e6d9dd84b4c47869c37c";

        // Load HTML content from file
        let html_content = fs::read_to_string("./test_data/lotr.html")
            .await
            .expect("Failed to read lotr.html");

        // Mock the server response
        let mock_server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::path(format!("/md5/{}", book_id)))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(html_content))
            .mount(&mock_server)
            .await;

        let mock_base_url = mock_server.uri();
        let book_info = get_book_info(book_id, Some(mock_base_url.as_str()))
            .await
            .expect("Failed to fetch book info");

        // Add assertions (update manually based on actual data from lotr.html)
        assert_eq!(book_info.id, book_id);
        assert!(book_info.title.contains("Lord of the Rings"));
        assert_eq!(book_info.author, Some("J. R. R. Tolkien 🔍".to_string()));
        assert_eq!(book_info.publisher, Some("cj5_7301".to_string()));
        assert!(book_info.download_urls.len() > 0);
    }

    #[test]
    async fn test_queue_book() {
        let book_id = "test_book_id";
        let book_info = BookInfo::new(book_id, "Test Book");

        // Queue the book
        queue_book(book_id, book_info.clone());

        // Verify the book is queued
        let status = get_queue_status();
        assert!(status
            .get(&QueueStatus::Queued)
            .map_or(false, |books| books.contains_key(book_id)));
    }

    #[test]
    async fn test_get_queue_status() {
        let book_id_1 = "book_1";
        let book_id_2 = "book_2";

        let book_info_1 = BookInfo::new(book_id_1, "Book 1");
        let book_info_2 = BookInfo::new(book_id_2, "Book 2");

        // Queue two books
        queue_book(book_id_1, book_info_1.clone());
        queue_book(book_id_2, book_info_2.clone());

        // Verify the status map
        let status = get_queue_status();

        assert!(status.get(&QueueStatus::Queued).is_some());
        let queued_books = status.get(&QueueStatus::Queued).unwrap();

        // Use `contains_key` instead of directly comparing `Option` values
        assert!(queued_books.contains_key(book_id_1));
        assert!(queued_books.contains_key(book_id_2));

        // Verify specific book information
        assert_eq!(
            queued_books.get(book_id_1).unwrap().title,
            book_info_1.title
        );
        assert_eq!(
            queued_books.get(book_id_2).unwrap().title,
            book_info_2.title
        );
    }

    #[test]
    async fn test_download_book_success() {
        // Set up a mock server
        let mock_server = MockServer::start().await;

        // Create a successful response for the mock server
        Mock::given(method("GET"))
            .and(path("/valid_url"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes("book data"))
            .mount(&mock_server)
            .await;

        // BookInfo with a valid download URL
        let book_info = BookInfo {
            id: "test_id".to_string(),
            title: "Test Book".to_string(),
            format: Some("epub".to_string()),
            download_urls: vec![format!("{}/valid_url", mock_server.uri())],
            ..Default::default()
        };

        // Call the function
        let result = download_book(&book_info).await;

        // Assert that the function completed successfully
        assert!(result.is_ok());

        // Assert the file was written to the expected path
        let expected_path = CONFIG.tmp_dir.join("test_id.epub");
        let content = tokio::fs::read_to_string(&expected_path).await.unwrap();
        assert_eq!(content, "book data");

        // Clean up
        tokio::fs::remove_file(expected_path).await.unwrap();
    }
}
