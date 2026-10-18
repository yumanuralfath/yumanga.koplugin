//! Manga sources. Every website the plugin can read from implements [`Source`].
//!
//! To add a source:
//!   1. create `sources/<name>.rs` with a type that implements `Source`
//!   2. add `pub mod <name>;` below
//!   3. return it from `all()`
//!      Nothing else (CLI, downloader, Lua) needs to change.

pub mod bacakomik;
pub mod komikindo;
pub mod komiku;
pub mod mangadex;
pub mod mangaku;
pub mod weebcentral;
pub mod westmanga;

use crate::models::{Chapter, PageImage, SearchResult, SeriesChapters};
use crate::util;

pub struct SearchOptions<'a> {
    /// Directory where cover images are cached ("" disables covers).
    pub cover_cache_dir: &'a str,
    /// Fetch per-series details (authors, tags...) for every result.
    pub with_details: bool,
    /// Optional source-supported genre name.
    pub genre: &'a str,
}

pub trait Source: Sync {
    /// Stable identifier used on the command line, e.g. "weebcentral".
    fn id(&self) -> &str;

    /// Human readable name shown in the UI.
    fn name(&self) -> &str;

    /// Filters supported by this source, advertised to the plugin UI.
    fn search_filters(&self) -> &'static [&'static str] {
        &["genre"]
    }

    fn search(&self, keyword: &str, options: &SearchOptions) -> Result<Vec<SearchResult>, String>;

    /// Series metadata plus its chapters.
    /// CONTRACT: `chapters` must be ordered oldest -> newest. Chapter numbers
    /// in the UI ("#1", selection strings like "1-10") are positions in this list.
    fn series(&self, series_id: &str) -> Result<SeriesChapters, String>;

    /// Every page image of a chapter, in reading order.
    fn pages(&self, chapter: &Chapter) -> Result<Vec<PageImage>, String>;

    /// Downloads one page image. Override when a site needs special headers
    /// or an image needs descrambling.
    fn fetch_image(&self, page: &PageImage) -> Result<Vec<u8>, String> {
        util::get_bytes(&page.url, page.referer.as_deref())
    }
}

/// All built-in sources. The first one is the default.
pub fn all() -> Vec<Box<dyn Source>> {
    vec![
        Box::new(weebcentral::WeebCentral),
        Box::new(bacakomik::BacaKomik),
        Box::new(komikindo::KomikIndo),
        Box::new(komiku::Komiku),
        Box::new(mangaku::Mangaku),
        Box::new(mangadex::MangaDex),
        Box::new(westmanga::WestManga),
    ]
}

pub fn find(id: &str) -> Option<Box<dyn Source>> {
    all().into_iter().find(|source| source.id() == id)
}

pub fn filter_results_by_genre(results: &mut Vec<SearchResult>, genre: &str) {
    let genre = genre.trim();
    if genre.is_empty() {
        return;
    }
    let genre = genre.to_lowercase();
    results.retain(|result| {
        result
            .tags
            .to_lowercase()
            .split(',')
            .any(|tag| tag.trim().contains(&genre))
    });
}

#[cfg(test)]
mod tests {
    use super::filter_results_by_genre;
    use crate::models::SearchResult;

    fn result(tags: &str) -> SearchResult {
        SearchResult {
            id: String::new(),
            title: String::new(),
            url: String::new(),
            cover_url: String::new(),
            cover_path: String::new(),
            authors: String::new(),
            tags: tags.to_string(),
            manga_type: String::new(),
            status: String::new(),
            released: String::new(),
        }
    }

    #[test]
    fn genre_filter_matches_tags_case_insensitively() {
        let mut results = vec![result("Action, Comedy"), result("Drama")];

        filter_results_by_genre(&mut results, "  comedy ");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].tags, "Action, Comedy");
    }
}
