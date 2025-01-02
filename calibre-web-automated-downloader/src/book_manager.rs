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
        title: cells[1].text().collect::<Vec<_>>().concat(),
        author: Some(cells[2].text().collect::<Vec<_>>().concat()),
        publisher: Some(cells[3].text().collect::<Vec<_>>().concat()),
        year: Some(cells[4].text().collect::<Vec<_>>().concat()),
        language: Some(cells[7].text().collect::<Vec<_>>().concat()),
        format: Some(cells[9].text().collect::<Vec<_>>().concat()),
        size: Some(cells[10].text().collect::<Vec<_>>().concat()),
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
    let data_selector = Selector::parse("body > main > div:nth-of-type(1)").unwrap();

    let data = document
        .select(&data_selector)
        .next()
        .ok_or_else(|| anyhow!("Failed to parse book info for ID: {}", book_id))?;

    let mut book_info = BookInfo {
        id: book_id.to_string(),
        title: extract_text(&data, "div:nth-of-type(1) > span")?,
        author: Some(extract_text(&data, "div:nth-of-type(2) > span")?),
        publisher: Some(extract_text(&data, "div:nth-of-type(3) > span")?),
        year: Some(extract_text(&data, "div:nth-of-type(4) > span")?),
        language: Some(extract_text(&data, "div:nth-of-type(5) > span")?),
        format: None,
        size: None,
        preview: None,
        info: Some(HashMap::new()),
        download_urls: vec![],
    };

    // Populate additional metadata and download URLs
    populate_metadata_and_urls(&data, &mut book_info)?;

    Ok(book_info)
}

/// Helper function to extract text content.
fn extract_text(data: &scraper::ElementRef, selector: &str) -> Result<String> {
    let element = data
        .select(&Selector::parse(selector).unwrap())
        .next()
        .ok_or_else(|| anyhow!("Failed to find element for selector: {}", selector))?;
    Ok(element.text().collect::<Vec<_>>().concat())
}

/// Populate additional metadata and download URLs.
fn populate_metadata_and_urls(data: &scraper::ElementRef, book_info: &mut BookInfo) -> Result<()> {
    let url_selector = Selector::parse("a").unwrap();

    for element in data.select(&url_selector) {
        if let Some(href) = element.value().attr("href") {
            book_info.download_urls.push(href.to_string());
        }
    }

    Ok(())
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
    use std::collections::HashMap;
    use tokio::test;
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

    // tests for get_book_info and its helpers
}
