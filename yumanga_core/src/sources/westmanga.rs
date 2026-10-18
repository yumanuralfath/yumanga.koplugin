//! westmanga.lol
use scraper::{Html, Selector};
use serde_json::Value;
use std::fs;
use std::path::Path;

use super::{SearchOptions, Source};
use crate::models::{Chapter, PageImage, SearchResult, SeriesChapters};
use crate::util::{USER_AGENT, absolute_url, get_text, normalize_whitespace, validate_id};

const BASE_URL: &str = "https://westmanga.lol";

pub struct WestManga;

impl Source for WestManga {
    fn id(&self) -> &'static str {
        "westmanga"
    }

    fn name(&self) -> &'static str {
        "WestManga"
    }

    fn search(&self, keyword: &str, options: &SearchOptions) -> Result<Vec<SearchResult>, String> {
        search(keyword, options.cover_cache_dir, options.with_details)
    }

    fn series(&self, series_id: &str) -> Result<SeriesChapters, String> {
        load_series(series_id)
    }

    fn pages(&self, chapter: &Chapter) -> Result<Vec<PageImage>, String> {
        let html = get_text(&chapter.url)?;
        parse_pages(chapter, &html)
    }
}

fn abs(base_url: &str, value: &str) -> String {
    absolute_url(base_url, value)
}

pub fn search(
    keyword: &str,
    cover_cache_dir: &str,
    with_details: bool,
) -> Result<Vec<SearchResult>, String> {
    search_for_site(BASE_URL, keyword, cover_cache_dir, with_details)
}

pub(crate) fn search_for_site(
    base_url: &str,
    keyword: &str,
    cover_cache_dir: &str,
    with_details: bool,
) -> Result<Vec<SearchResult>, String> {
    let response = ureq::get(&format!("{base_url}/manga"))
        .query("search", keyword)
        .set("User-Agent", USER_AGENT)
        .call()
        .map_err(|error| error.to_string())?;
    let html = crate::util::read_response_text(response)?;
    let props = inertia_props(&html)?;
    let data = props
        .get("mangas")
        .and_then(|mangas| mangas.get("data"))
        .and_then(Value::as_array)
        .ok_or_else(|| "Manga search results were missing".to_string())?;
    let mut results = parse_search_results_for_site(base_url, data, with_details);
    if !cover_cache_dir.is_empty() {
        fs::create_dir_all(cover_cache_dir).map_err(|error| error.to_string())?;
    }
    for result in &mut results {
        result.cover_path = cache_cover(&result.id, &result.cover_url, Path::new(cover_cache_dir));
    }
    Ok(results)
}

pub(crate) fn parse_search_results_for_site(
    base_url: &str,
    mangas: &[Value],
    with_details: bool,
) -> Vec<SearchResult> {
    mangas
        .iter()
        .filter_map(|manga| {
            let id = string_value(manga.get("slug"))?;
            let title = string_value(manga.get("title"))?;
            let cover_url = manga
                .get("poster")
                .and_then(Value::as_str)
                .map(|poster| abs(base_url, poster))
                .unwrap_or_default();
            let (authors, tags, manga_type, status, released) = if with_details {
                metadata(manga)
            } else {
                Default::default()
            };
            Some(SearchResult {
                id: id.to_string(),
                title: title.to_string(),
                url: format!("{base_url}/manga/{id}"),
                cover_url,
                cover_path: String::new(),
                authors,
                tags,
                manga_type,
                status,
                released,
            })
        })
        .collect()
}

fn metadata(manga: &Value) -> (String, String, String, String, String) {
    let mut authors = Vec::new();
    for field in ["author", "artist"] {
        if let Some(value) = manga.get(field).and_then(Value::as_str) {
            let value = value.trim();
            if !value.is_empty()
                && value != "-"
                && !authors.iter().any(|existing| existing == value)
            {
                authors.push(value.to_string());
            }
        }
    }
    let tags = manga
        .get("genres")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|genre| genre.get("name").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join(", ");
    (
        authors.join(", "),
        tags,
        value_string(manga.get("type")),
        value_string(manga.get("status")),
        value_string(manga.get("release_year")),
    )
}

pub fn load_series(series_id: &str) -> Result<SeriesChapters, String> {
    load_series_for_site(BASE_URL, series_id)
}

pub(crate) fn load_series_for_site(
    base_url: &str,
    series_id: &str,
) -> Result<SeriesChapters, String> {
    validate_id(series_id)?;
    let html = get_text(&format!("{base_url}/manga/{series_id}"))?;
    let props = inertia_props(&html)?;
    parse_series_for_site(base_url, series_id, &props)
}

pub(crate) fn parse_series_for_site(
    base_url: &str,
    series_id: &str,
    props: &Value,
) -> Result<SeriesChapters, String> {
    let manga = props
        .get("manga")
        .ok_or_else(|| "Manga series data was missing".to_string())?;
    let title = value_string(manga.get("title"));
    let (authors, tags, manga_type, status, released) = metadata(manga);
    let cover_url = manga
        .get("poster")
        .and_then(Value::as_str)
        .map(|poster| abs(base_url, poster))
        .unwrap_or_default();
    let description = manga
        .get("synopsis")
        .and_then(Value::as_str)
        .map(|synopsis| normalize_whitespace(synopsis.to_string()))
        .unwrap_or_default();
    let chapters = manga
        .get("chapters")
        .and_then(Value::as_array)
        .map(|chapters| parse_chapters(base_url, series_id, chapters))
        .unwrap_or_default();

    Ok(SeriesChapters {
        id: series_id.to_string(),
        title,
        url: format!("{base_url}/manga/{series_id}"),
        cover_url,
        authors,
        tags,
        manga_type,
        status,
        released,
        description,
        chapters,
    })
}

fn parse_chapters(base_url: &str, series_slug: &str, chapters: &[Value]) -> Vec<Chapter> {
    let mut parsed = chapters
        .iter()
        .filter_map(|chapter| {
            let number = chapter.get("chapter_number")?;
            let number_string = number
                .as_f64()
                .map(|number| number.to_string())
                .or_else(|| number.as_str().map(str::to_string))?;
            let title = string_value(chapter.get("title"))?;
            let slug = string_value(chapter.get("slug")).unwrap_or(&number_string);
            let url = format!("{base_url}/manga/{series_slug}/chapter/{number_string}");
            let published_at = value_string(chapter.get("created_at"));
            Some((
                number.as_f64().unwrap_or_default(),
                Chapter {
                    id: slug.to_string(),
                    title: title.to_string(),
                    url,
                    published_at,
                },
            ))
        })
        .collect::<Vec<_>>();
    parsed.sort_by(|left, right| left.0.total_cmp(&right.0));
    parsed.into_iter().map(|(_, chapter)| chapter).collect()
}

fn parse_pages(chapter: &Chapter, chapter_html: &str) -> Result<Vec<PageImage>, String> {
    parse_pages_for_site(BASE_URL, chapter, chapter_html)
}

pub(crate) fn parse_pages_for_site(
    base_url: &str,
    chapter: &Chapter,
    chapter_html: &str,
) -> Result<Vec<PageImage>, String> {
    let props = inertia_props(chapter_html)?;
    let images = props
        .get("chapter")
        .and_then(|chapter| chapter.get("images"))
        .and_then(Value::as_array)
        .ok_or_else(|| "Manga chapter images were missing".to_string())?;
    let mut images = images.iter().collect::<Vec<_>>();
    images.sort_by(|left, right| {
        numeric_value(left.get("order")).total_cmp(&numeric_value(right.get("order")))
    });
    let pages = images
        .into_iter()
        .filter_map(|image| image.get("image_path").and_then(Value::as_str))
        .filter(|path| !is_thumbnail(path))
        .map(|path| PageImage {
            url: abs(base_url, path),
            referer: Some(chapter.url.clone()),
            headers: Default::default(),
            context: None,
        })
        .filter(|page| !page.url.is_empty())
        .collect::<Vec<_>>();
    if pages.is_empty() {
        return Err("Could not find chapter page images".to_string());
    }
    Ok(pages)
}

fn is_thumbnail(path: &str) -> bool {
    let path = path.to_ascii_lowercase();
    [
        "150x150", "300x300", "thumb", "emoji", "icon", "avatar", "logo", "banner", "promosi",
        "-0x0", "-45x45",
    ]
    .iter()
    .any(|marker| path.contains(marker))
}

pub(crate) fn inertia_props(html: &str) -> Result<Value, String> {
    let document = Html::parse_document(html);
    let selector = Selector::parse("#app[data-page]").expect("valid Inertia payload selector");
    let payload = document
        .select(&selector)
        .next()
        .and_then(|element| element.value().attr("data-page"))
        .ok_or_else(|| "Could not find manga site page data".to_string())?;
    let page: Value = serde_json::from_str(payload).map_err(|error| error.to_string())?;
    page.get("props")
        .cloned()
        .ok_or_else(|| "Manga site page properties were missing".to_string())
}

fn string_value(value: Option<&Value>) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
}

fn value_string(value: Option<&Value>) -> String {
    value
        .and_then(|value| match value {
            Value::String(value) => Some(value.clone()),
            Value::Number(value) => Some(value.to_string()),
            _ => None,
        })
        .unwrap_or_default()
}

fn numeric_value(value: Option<&Value>) -> f64 {
    value
        .and_then(Value::as_f64)
        .or_else(|| {
            value
                .and_then(Value::as_str)
                .and_then(|value| value.parse().ok())
        })
        .unwrap_or_default()
}

fn cache_cover(id: &str, cover_url: &str, cache_dir: &Path) -> String {
    if cover_url.is_empty() || cache_dir.as_os_str().is_empty() {
        return String::new();
    }
    let extension = cover_url
        .split(['?', '#'])
        .next()
        .and_then(|url| url.rsplit('.').next())
        .filter(|extension| {
            extension.len() <= 5
                && extension
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric())
        })
        .unwrap_or("img");
    let path = cache_dir.join(format!("{id}.{extension}"));
    if fs::metadata(&path)
        .map(|meta| meta.len() > 0)
        .unwrap_or(false)
    {
        return path.to_string_lossy().into_owned();
    }
    let response = match ureq::get(cover_url).set("User-Agent", USER_AGENT).call() {
        Ok(response) => response,
        Err(_) => return String::new(),
    };
    let mut reader = response.into_reader();
    let mut output = match fs::File::create(&path) {
        Ok(output) => output,
        Err(_) => return String::new(),
    };
    if std::io::copy(&mut reader, &mut output).is_err() {
        let _ = fs::remove_file(&path);
        return String::new();
    }
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
#[path = "../../tests/unit/westmanga/mod.rs"]
mod tests;
