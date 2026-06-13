use reqwest::Client;

pub fn build_client() -> Result<Client, reqwest::Error> {
    Client::builder()
        .user_agent("Mozilla/5.0 (compatible; Ferrico/1.0; +https://ferrico.app)")
        .timeout(std::time::Duration::from_secs(15))
        .build()
}

/// Fetches the og:image or twitter:image meta tag URL from a webpage.
/// Returns None if the page can't be fetched or has no such tag.
pub async fn fetch_og_image(client: &Client, url: &str) -> Option<String> {
    let html = client
        .get(url)
        .send()
        .await
        .ok()?
        .text()
        .await
        .ok()?;

    let doc = scraper::Html::parse_document(&html);

    // Try og:image first, fall back to twitter:image
    for selector_str in &[
        "meta[property='og:image']",
        "meta[name='og:image']",
        "meta[name='twitter:image']",
        "meta[property='twitter:image']",
    ] {
        if let Ok(sel) = scraper::Selector::parse(selector_str) {
            if let Some(el) = doc.select(&sel).next() {
                if let Some(content) = el.value().attr("content") {
                    let trimmed = content.trim().to_string();
                    if !trimmed.is_empty() {
                        return Some(trimmed);
                    }
                }
            }
        }
    }

    None
}
