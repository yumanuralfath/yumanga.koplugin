use crate::models::{Chapter, PageImage, SearchResult, SeriesChapters};
use crate::sources::{SearchOptions, Source};

pub(super) struct MockSource;

impl Source for MockSource {
    fn id(&self) -> &'static str {
        "mock"
    }

    fn name(&self) -> &'static str {
        "Mock"
    }

    fn search(&self, _: &str, _: &SearchOptions) -> Result<Vec<SearchResult>, String> {
        Ok(Vec::new())
    }

    fn series(&self, series_id: &str) -> Result<SeriesChapters, String> {
        let chapters = (1..=3)
            .map(|number| Chapter {
                id: format!("c{number}"),
                title: format!("Chapter {number}"),
                url: format!("mock://chapter/{number}"),
                published_at: String::new(),
            })
            .collect();
        Ok(SeriesChapters {
            id: series_id.to_string(),
            title: "Mock Manga".to_string(),
            url: String::new(),
            cover_url: String::new(),
            authors: String::new(),
            tags: String::new(),
            manga_type: String::new(),
            status: String::new(),
            released: String::new(),
            description: String::new(),
            chapters,
        })
    }

    fn pages(&self, chapter: &Chapter) -> Result<Vec<PageImage>, String> {
        Ok((1..=2)
            .map(|number| PageImage {
                url: format!("{}/{number}.png", chapter.url),
                referer: None,
                headers: Default::default(),
                context: None,
            })
            .collect())
    }

    fn fetch_image(&self, page: &PageImage) -> Result<Vec<u8>, String> {
        Ok(page.url.clone().into_bytes())
    }
}

pub(super) fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("wc_test_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

pub(super) fn entry_names(path: &str) -> Vec<String> {
    let mut archive = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    (0..archive.len())
        .map(|index| archive.by_index(index).unwrap().name().to_string())
        .collect()
}
