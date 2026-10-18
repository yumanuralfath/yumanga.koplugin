//! komiku.org
use scraper::{Html, Selector};
use std::fs;
use std::path::Path;

use super::{SearchOptions, Source};
use crate::models::{Chapter, PageImage, SearchResult, SeriesChapters};
use crate::util::{USER_AGENT, absolute_url, get_text, normalize_whitespace, read_response_text};

const BASE_URL: &str = "https://komiku.org";
const SEARCH_URL: &str = "https://api.komiku.org/";

pub struct Komiku;

impl Source for Komiku {
    fn id(&self) -> &'static str {
        "komiku"
    }

    fn name(&self) -> &'static str {
        "Komiku"
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

fn abs(value: &str) -> String {
    absolute_url(BASE_URL, value)
}

pub fn search(
    keyword: &str,
    cover_cache_dir: &str,
    with_details: bool,
) -> Result<Vec<SearchResult>, String> {
    let response = ureq::get(SEARCH_URL)
        .query("post_type", "manga")
        .query("s", keyword)
        .set("User-Agent", USER_AGENT)
        .call()
        .map_err(|error| error.to_string())?;
    let html = read_response_text(response)?;
    let mut results = parse_search_results(&Html::parse_document(&html));
    if !cover_cache_dir.is_empty() {
        fs::create_dir_all(cover_cache_dir).map_err(|error| error.to_string())?;
    }
    for result in &mut results {
        result.cover_path = cache_cover(&result.id, &result.cover_url, Path::new(cover_cache_dir));
        if with_details {
            if let Ok(html) = get_text(&result.url) {
                apply_metadata(
                    &mut result.authors,
                    &mut result.tags,
                    &mut result.manga_type,
                    &mut result.status,
                    &mut result.released,
                    &Html::parse_document(&html),
                );
            }
        }
    }
    Ok(results)
}

fn parse_search_results(document: &Html) -> Vec<SearchResult> {
    let card_selector = Selector::parse(".bge").expect("valid manga card selector");
    let title_selector = Selector::parse(".kan > a").expect("valid manga title selector");
    let image_selector = Selector::parse(".bgei img").expect("valid cover image selector");
    document
        .select(&card_selector)
        .filter_map(|card| {
            let title_link = card.select(&title_selector).next()?;
            let href = title_link.value().attr("href").unwrap_or_default();
            let id = slug_from_href(href, "/manga/")?;
            let title = normalize_whitespace(title_link.text().collect::<Vec<_>>().join(" "));
            if title.is_empty() {
                return None;
            }
            let cover_url = card
                .select(&image_selector)
                .next()
                .and_then(|image| {
                    image
                        .value()
                        .attr("data-src")
                        .or_else(|| image.value().attr("src"))
                })
                .map(abs)
                .unwrap_or_default();
            Some(SearchResult {
                id: id.to_string(),
                title,
                url: abs(href),
                cover_url,
                cover_path: String::new(),
                authors: String::new(),
                tags: String::new(),
                manga_type: String::new(),
                status: String::new(),
                released: String::new(),
            })
        })
        .collect()
}

pub fn load_series(series_id: &str) -> Result<SeriesChapters, String> {
    validate_komiku_slug(series_id)?;
    let url = format!("{BASE_URL}/manga/{series_id}/");
    let document = Html::parse_document(&get_text(&url)?);
    parse_series(series_id, &url, &document)
}

fn parse_series(series_id: &str, url: &str, document: &Html) -> Result<SeriesChapters, String> {
    let title_selector =
        Selector::parse("h1 span[itemprop='name']").expect("valid series title selector");
    let title = document
        .select(&title_selector)
        .next()
        .map(|element| normalize_whitespace(element.text().collect::<Vec<_>>().join(" ")))
        .unwrap_or_default();
    let cover_selector =
        Selector::parse("#Informasi .ims img").expect("valid series cover selector");
    let cover_url = document
        .select(&cover_selector)
        .next()
        .and_then(|image| {
            image
                .value()
                .attr("data-src")
                .or_else(|| image.value().attr("src"))
        })
        .map(abs)
        .unwrap_or_default();
    let description_selector = Selector::parse("p.desc").expect("valid synopsis selector");
    let description = document
        .select(&description_selector)
        .next()
        .map(|paragraph| normalize_whitespace(paragraph.text().collect::<Vec<_>>().join(" ")))
        .unwrap_or_default();
    let mut authors = String::new();
    let mut tags = String::new();
    let mut manga_type = String::new();
    let mut status = String::new();
    let mut released = String::new();
    apply_metadata(
        &mut authors,
        &mut tags,
        &mut manga_type,
        &mut status,
        &mut released,
        &document,
    );
    let chapters = parse_chapters(&document);

    Ok(SeriesChapters {
        id: series_id.to_string(),
        title,
        url: url.to_string(),
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

fn apply_metadata(
    authors: &mut String,
    tags: &mut String,
    manga_type: &mut String,
    status: &mut String,
    released: &mut String,
    document: &Html,
) {
    let row_selector = Selector::parse("table.inftable tr").expect("valid info row selector");
    let cell_selector = Selector::parse("td").expect("valid info cell selector");
    let link_selector = Selector::parse("a").expect("valid genre link selector");
    for row in document.select(&row_selector) {
        let cells = row.select(&cell_selector).collect::<Vec<_>>();
        if cells.len() < 2 {
            continue;
        }
        let label = normalize_whitespace(cells[0].text().collect::<Vec<_>>().join(" "))
            .trim_end_matches(':')
            .to_ascii_lowercase();
        let value = normalize_whitespace(cells[1].text().collect::<Vec<_>>().join(" "));
        match label.as_str() {
            "author" | "pengarang" => *authors = value,
            "tipe" | "type" => *manga_type = value,
            "status" => *status = value,
            "tahun" | "tahun rilis" | "released" => *released = value,
            "genre" => {
                *tags = cells[1]
                    .select(&link_selector)
                    .map(|link| normalize_whitespace(link.text().collect::<Vec<_>>().join(" ")))
                    .filter(|name| !name.is_empty())
                    .collect::<Vec<_>>()
                    .join(", ");
            }
            _ => {}
        }
    }
}

fn parse_chapters(document: &Html) -> Vec<Chapter> {
    let row_selector = Selector::parse("#daftarChapter tr[itemprop='itemListElement']")
        .expect("valid chapter row selector");
    let link_selector = Selector::parse("a[itemprop='url']").expect("valid chapter link selector");
    let title_selector =
        Selector::parse("[itemprop='name']").expect("valid chapter title selector");
    let date_selector = Selector::parse(".tanggalseries").expect("valid chapter date selector");
    let mut chapters = Vec::new();
    for row in document.select(&row_selector) {
        let Some(link) = row.select(&link_selector).next() else {
            continue;
        };
        let href = link.value().attr("href").unwrap_or_default();
        let Some(id) = slug_from_href(href, "/") else {
            continue;
        };
        let title = row
            .select(&title_selector)
            .next()
            .map(|element| normalize_whitespace(element.text().collect::<Vec<_>>().join(" ")))
            .unwrap_or_default();
        if title.is_empty() {
            continue;
        }
        let published_at = row
            .select(&date_selector)
            .next()
            .map(|date| normalize_whitespace(date.text().collect::<Vec<_>>().join(" ")))
            .unwrap_or_default();
        chapters.push(Chapter {
            id: id.to_string(),
            title,
            url: abs(href),
            published_at,
        });
    }
    chapters.reverse();
    chapters
}

fn parse_pages(chapter: &Chapter, chapter_html: &str) -> Result<Vec<PageImage>, String> {
    let document = Html::parse_document(chapter_html);
    let image_selector = Selector::parse("img.klazy.ww").expect("valid chapter image selector");
    let pages = document
        .select(&image_selector)
        .filter_map(|image| {
            image
                .value()
                .attr("data-src")
                .or_else(|| image.value().attr("src"))
        })
        .filter(|url| !url.is_empty())
        .map(|url| PageImage {
            url: abs(url),
            referer: Some(chapter.url.clone()),
            headers: Default::default(),
            context: None,
        })
        .collect::<Vec<_>>();
    if pages.is_empty() {
        return Err("Could not find Komiku chapter page images".to_string());
    }
    Ok(pages)
}

fn slug_from_href<'a>(href: &'a str, route: &str) -> Option<&'a str> {
    let path = href.strip_prefix(BASE_URL).unwrap_or(href);
    let path = path.strip_prefix(route)?;
    let slug = path.split(['?', '#']).next()?.trim_matches('/');
    (!slug.is_empty()).then_some(slug)
}

fn validate_komiku_slug(slug: &str) -> Result<(), String> {
    if slug.is_empty() {
        return Err("Invalid Komiku manga ID".to_string());
    }
    let bytes = slug.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len()
                && bytes[index + 1].is_ascii_hexdigit()
                && bytes[index + 2].is_ascii_hexdigit() =>
            {
                let high = (bytes[index + 1] as char).to_digit(16).unwrap_or_default() as u8;
                let low = (bytes[index + 2] as char).to_digit(16).unwrap_or_default() as u8;
                let decoded = (high << 4) | low;
                if matches!(decoded, b'/' | b'\\' | b'?' | b'#') {
                    return Err("Invalid Komiku manga ID".to_string());
                }
                index += 3;
            }
            byte if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.') => {
                index += 1;
            }
            _ => return Err("Invalid Komiku manga ID".to_string()),
        }
    }
    if slug == "." || slug == ".." {
        return Err("Invalid Komiku manga ID".to_string());
    }
    Ok(())
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
#[path = "../../tests/unit/komiku/mod.rs"]
mod tests;
