//! mangadex.org
use serde_json::Value;
use std::fs;
use std::path::Path;

use super::{SearchOptions, Source};
use crate::models::{Chapter, PageImage, SearchResult, SeriesChapters};
use crate::util::{USER_AGENT, validate_id};

const API_URL: &str = "https://api.mangadex.org";
const SITE_URL: &str = "https://mangadex.org";
const API_USER_AGENT: &str = "YuMangaDownloader/0.1.0 (https://github.com/yuma/yumanga)";

pub struct MangaDex;

impl Source for MangaDex {
    fn id(&self) -> &'static str {
        "mangadex"
    }

    fn name(&self) -> &'static str {
        "MangaDex"
    }

    fn search_filters(&self) -> &'static [&'static str] {
        &["genre"]
    }

    fn search(&self, keyword: &str, options: &SearchOptions) -> Result<Vec<SearchResult>, String> {
        search_with_genre(
            keyword,
            options.cover_cache_dir,
            options.with_details,
            options.genre,
        )
    }

    fn series(&self, series_id: &str) -> Result<SeriesChapters, String> {
        load_series(series_id)
    }

    fn pages(&self, chapter: &Chapter) -> Result<Vec<PageImage>, String> {
        load_pages(&chapter.id)
    }
}

pub fn search_with_genre(
    keyword: &str,
    cover_cache_dir: &str,
    with_details: bool,
    genre: &str,
) -> Result<Vec<SearchResult>, String> {
    let tag_id = if genre.trim().is_empty() {
        None
    } else {
        let tags = api_json(&format!("{API_URL}/manga/tag"), &[])?;
        Some(find_tag_id(&tags, genre.trim())?)
    };
    let query = search_query(keyword, tag_id.as_deref());
    let mut request = ureq::get(&format!("{API_URL}/manga"))
        .set("User-Agent", API_USER_AGENT)
        .set("Accept", "application/json");
    for (key, value) in &query {
        request = request.query(key, value);
    }
    let response = request.call().map_err(|error| error.to_string())?;
    let payload: Value = crate::util::read_response_text(response)
        .and_then(|body| serde_json::from_str(&body).map_err(|error| error.to_string()))?;
    let mangas = payload
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| "MangaDex search results were missing".to_string())?;
    let mut results = parse_search_results(mangas, with_details);
    if !cover_cache_dir.is_empty() {
        fs::create_dir_all(cover_cache_dir).map_err(|error| error.to_string())?;
    }
    for result in &mut results {
        result.cover_path = cache_cover(&result.id, &result.cover_url, Path::new(cover_cache_dir));
    }
    Ok(results)
}

fn search_query(keyword: &str, tag_id: Option<&str>) -> Vec<(String, String)> {
    let mut query = vec![
        ("limit".to_string(), "10".to_string()),
        ("includes[]".to_string(), "cover_art".to_string()),
        ("includes[]".to_string(), "author".to_string()),
        ("includes[]".to_string(), "artist".to_string()),
        ("contentRating[]".to_string(), "safe".to_string()),
        ("contentRating[]".to_string(), "suggestive".to_string()),
    ];
    if !keyword.trim().is_empty() {
        query.push(("title".to_string(), keyword.to_string()));
    }
    if let Some(tag_id) = tag_id {
        query.push(("includedTags[]".to_string(), tag_id.to_string()));
    }
    query
}

fn find_tag_id(payload: &Value, genre: &str) -> Result<String, String> {
    payload
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|tag| {
            tag.get("attributes")
                .and_then(|attributes| attributes.get("name"))
                .and_then(|names| names.get("en"))
                .and_then(Value::as_str)
                .is_some_and(|name| name.eq_ignore_ascii_case(genre))
        })
        .and_then(|tag| tag.get("id"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("MangaDex genre not found: {genre}"))
}

fn parse_search_results(mangas: &[Value], with_details: bool) -> Vec<SearchResult> {
    mangas
        .iter()
        .filter_map(|manga| {
            let id = manga.get("id").and_then(Value::as_str)?;
            let attributes = manga.get("attributes")?;
            let title = localized_value(attributes.get("title"))?;
            let cover_url = cover_url(id, manga);
            let (authors, tags, manga_type, status, released) = if with_details {
                metadata(manga)
            } else {
                Default::default()
            };
            Some(SearchResult {
                id: id.to_string(),
                title,
                url: format!("{SITE_URL}/title/{id}"),
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

pub fn load_series(series_id: &str) -> Result<SeriesChapters, String> {
    validate_id(series_id)?;
    let manga_payload = api_json(
        &format!("{API_URL}/manga/{series_id}"),
        &[
            ("includes[]", "cover_art"),
            ("includes[]", "author"),
            ("includes[]", "artist"),
        ],
    )?;
    let manga = manga_payload
        .get("data")
        .ok_or_else(|| "MangaDex series data was missing".to_string())?;
    let chapters = load_chapters(series_id)?;

    Ok(load_series_data(series_id, manga, chapters))
}

fn prefer_indonesian_translations(chapters: Vec<Value>) -> Vec<Value> {
    let indonesian_chapters = chapters
        .iter()
        .filter(|chapter| {
            chapter
                .get("attributes")
                .and_then(|attributes| attributes.get("translatedLanguage"))
                .and_then(Value::as_str)
                == Some("id")
        })
        .filter_map(chapter_identity)
        .collect::<std::collections::HashSet<_>>();

    chapters
        .into_iter()
        .filter(|chapter| {
            let Some(attributes) = chapter.get("attributes") else {
                return false;
            };
            if !has_readable_pages(attributes) {
                return false;
            }
            let is_english =
                attributes.get("translatedLanguage").and_then(Value::as_str) == Some("en");
            !is_english
                || chapter_identity(chapter)
                    .is_none_or(|identity| !indonesian_chapters.contains(&identity))
        })
        .collect()
}

fn chapter_identity(chapter: &Value) -> Option<(Option<String>, String)> {
    let attributes = chapter.get("attributes")?;
    let number = string_value(attributes.get("chapter"))?.to_string();
    let volume = string_value(attributes.get("volume")).map(str::to_string);
    Some((volume, number))
}

fn load_chapters(series_id: &str) -> Result<Vec<Chapter>, String> {
    let mut chapter_entries = Vec::new();
    let mut offset = 0;
    loop {
        let offset_string = offset.to_string();
        let query = vec![
            ("limit", "500"),
            ("offset", offset_string.as_str()),
            ("translatedLanguage[]", "id"),
            ("translatedLanguage[]", "en"),
            ("order[volume]", "asc"),
            ("order[chapter]", "asc"),
            ("includes[]", "scanlation_group"),
        ];
        let feed_payload = api_json(&format!("{API_URL}/manga/{series_id}/feed"), &query)?;
        let page = feed_payload
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(|| "MangaDex chapter feed was missing".to_string())?;
        chapter_entries.extend(page.iter().cloned());
        let total = feed_payload
            .get("total")
            .and_then(Value::as_u64)
            .unwrap_or(offset as u64 + page.len() as u64) as usize;
        offset += page.len();
        if page.is_empty() || offset >= total {
            break;
        }
    }

    Ok(parse_chapters(&prefer_indonesian_translations(
        chapter_entries,
    )))
}

fn load_series_data(series_id: &str, manga: &Value, chapters: Vec<Chapter>) -> SeriesChapters {
    let (authors, tags, manga_type, status, released) = metadata(manga);
    let description = manga
        .get("attributes")
        .and_then(|attributes| attributes.get("description"))
        .and_then(|description| localized_value(Some(description)))
        .unwrap_or_default();
    SeriesChapters {
        id: series_id.to_string(),
        title: manga
            .get("attributes")
            .and_then(|attributes| localized_value(attributes.get("title")))
            .unwrap_or_default(),
        url: format!("{SITE_URL}/title/{series_id}"),
        cover_url: cover_url(series_id, manga),
        authors,
        tags,
        manga_type,
        status,
        released,
        description,
        chapters,
    }
}

fn parse_chapters(chapters: &[Value]) -> Vec<Chapter> {
    chapters
        .iter()
        .filter_map(|chapter| {
            let attributes = chapter.get("attributes")?;
            if !has_readable_pages(attributes) {
                return None;
            }
            let id = chapter.get("id").and_then(Value::as_str)?;
            let chapter_number = string_value(attributes.get("chapter"));
            let volume = string_value(attributes.get("volume"));
            let title = string_value(attributes.get("title"));
            let mut display_title = match (volume, chapter_number) {
                (Some(volume), Some(number)) => format!("Vol. {volume} Ch. {number}"),
                (_, Some(number)) => format!("Chapter {number}"),
                (Some(volume), None) => format!("Volume {volume}"),
                (None, None) => "Special chapter".to_string(),
            };
            if let Some(title) = title {
                display_title.push_str(" - ");
                display_title.push_str(title);
            }
            let groups = chapter
                .get("relationships")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|relationship| {
                    relationship.get("type").and_then(Value::as_str) == Some("scanlation_group")
                })
                .filter_map(|relationship| {
                    relationship
                        .get("attributes")
                        .and_then(|attributes| attributes.get("name"))
                        .and_then(Value::as_str)
                })
                .collect::<Vec<_>>();
            if !groups.is_empty() {
                display_title.push_str(" [");
                display_title.push_str(&groups.join(", "));
                display_title.push(']');
            }
            let published_at = attributes
                .get("publishAt")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            Some(Chapter {
                id: id.to_string(),
                title: display_title,
                url: format!("{SITE_URL}/chapter/{id}"),
                published_at,
            })
        })
        .collect()
}

fn has_readable_pages(attributes: &Value) -> bool {
    attributes
        .get("pages")
        .and_then(Value::as_u64)
        .unwrap_or_default()
        > 0
        && !attributes
            .get("externalUrl")
            .is_some_and(|url| !url.is_null())
        && !attributes
            .get("isUnavailable")
            .and_then(Value::as_bool)
            .unwrap_or(false)
}

fn load_pages(chapter_id: &str) -> Result<Vec<PageImage>, String> {
    validate_id(chapter_id)?;
    let payload = api_json(&format!("{API_URL}/at-home/server/{chapter_id}"), &[])?;
    parse_at_home_pages(chapter_id, &payload)
}

fn parse_at_home_pages(chapter_id: &str, payload: &Value) -> Result<Vec<PageImage>, String> {
    let base_url = payload
        .get("baseUrl")
        .and_then(Value::as_str)
        .ok_or_else(|| "MangaDex image server URL was missing".to_string())?;
    let chapter = payload
        .get("chapter")
        .ok_or_else(|| "MangaDex chapter image data was missing".to_string())?;
    let hash = chapter
        .get("hash")
        .and_then(Value::as_str)
        .ok_or_else(|| "MangaDex chapter image hash was missing".to_string())?;
    let filenames = chapter
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| "MangaDex chapter page list was missing".to_string())?;
    let referer = format!("{SITE_URL}/chapter/{chapter_id}");
    let pages = filenames
        .iter()
        .filter_map(Value::as_str)
        .map(|filename| PageImage {
            url: format!("{base_url}/data/{hash}/{filename}"),
            referer: Some(referer.clone()),
            headers: Default::default(),
            context: None,
        })
        .collect::<Vec<_>>();
    if pages.is_empty() {
        return Err("MangaDex chapter has no image pages".to_string());
    }
    Ok(pages)
}

fn api_json(url: &str, query: &[(&str, &str)]) -> Result<Value, String> {
    let mut request = ureq::get(url)
        .set("User-Agent", API_USER_AGENT)
        .set("Accept", "application/json");
    for (key, value) in query {
        request = request.query(key, value);
    }
    let response = request.call().map_err(|error| error.to_string())?;
    let body = crate::util::read_response_text(response)?;
    serde_json::from_str(&body).map_err(|error| error.to_string())
}

fn metadata(manga: &Value) -> (String, String, String, String, String) {
    let attributes = manga.get("attributes");
    let mut authors = Vec::new();
    for name in manga
        .get("relationships")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|relationship| {
            matches!(
                relationship.get("type").and_then(Value::as_str),
                Some("author" | "artist")
            )
        })
        .filter_map(|relationship| {
            relationship
                .get("attributes")
                .and_then(|attributes| attributes.get("name"))
                .and_then(Value::as_str)
        })
    {
        if !authors.contains(&name) {
            authors.push(name);
        }
    }
    let tags = attributes
        .and_then(|attributes| attributes.get("tags"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|tag| {
            tag.get("attributes")
                .and_then(|attributes| localized_value(attributes.get("name")))
        })
        .collect::<Vec<_>>()
        .join(", ");
    let manga_type = match attributes
        .and_then(|attributes| attributes.get("originalLanguage"))
        .and_then(Value::as_str)
    {
        Some("ja") => "Manga",
        Some("ko") => "Manhwa",
        Some("zh") | Some("zh-hk") => "Manhua",
        _ => "",
    }
    .to_string();
    (
        authors.join(", "),
        tags,
        manga_type,
        attributes
            .and_then(|attributes| attributes.get("status"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        attributes
            .and_then(|attributes| attributes.get("year"))
            .and_then(|year| match year {
                Value::Number(year) => Some(year.to_string()),
                Value::String(year) => Some(year.clone()),
                _ => None,
            })
            .unwrap_or_default(),
    )
}

fn cover_url(manga_id: &str, manga: &Value) -> String {
    let filename = manga
        .get("relationships")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|relationship| relationship.get("type").and_then(Value::as_str) == Some("cover_art"))
        .and_then(|relationship| relationship.get("attributes"))
        .and_then(|attributes| attributes.get("fileName"))
        .and_then(Value::as_str);
    filename
        .map(|filename| {
            format!("https://uploads.mangadex.org/covers/{manga_id}/{filename}.256.jpg")
        })
        .unwrap_or_default()
}

fn localized_value(value: Option<&Value>) -> Option<String> {
    let translations = value?.as_object()?;
    ["en", "ja-ro", "ja"]
        .iter()
        .find_map(|language| translations.get(*language).and_then(Value::as_str))
        .or_else(|| translations.values().find_map(Value::as_str))
        .filter(|text| !text.trim().is_empty())
        .map(str::to_string)
}

fn string_value(value: Option<&Value>) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
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
#[path = "../../tests/unit/mangadex/mod.rs"]
mod tests;
