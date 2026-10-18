use serde::Serialize;
use std::collections::HashMap;

#[derive(Serialize)]
pub struct SearchResult {
    pub id: String,
    pub title: String,
    pub url: String,
    pub cover_url: String,
    pub cover_path: String,
    pub authors: String,
    pub tags: String,
    pub manga_type: String,
    pub status: String,
    pub released: String,
}

#[derive(Serialize, Debug)]
pub struct Chapter {
    pub id: String,
    pub title: String,
    pub url: String,
    pub published_at: String,
}

#[derive(Serialize, Debug)]
pub struct SeriesChapters {
    pub id: String,
    pub title: String,
    pub url: String,
    pub cover_url: String,
    pub authors: String,
    pub tags: String,
    pub manga_type: String,
    pub status: String,
    pub released: String,
    pub description: String,
    pub chapters: Vec<Chapter>,
}

#[derive(Serialize)]
pub struct DownloadResult {
    pub paths: Vec<String>,
    pub skipped: usize,
}

/// One page image of a chapter.
#[derive(Debug, Clone, PartialEq)]
pub struct PageImage {
    pub url: String,
    /// Sent as the `Referer` header when set (many sites hotlink-protect images).
    pub referer: Option<String>,
    pub headers: HashMap<String, String>,
    pub context: Option<HashMap<String, String>>,
}

#[derive(Serialize)]
pub struct SourceInfo {
    pub id: String,
    pub name: String,
    pub search_filters: Vec<String>,
}
