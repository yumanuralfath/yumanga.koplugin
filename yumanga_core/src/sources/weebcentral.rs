//! weebcentral.com
use regex::Regex;
use scraper::{Html, Selector};
use std::fs;
use std::path::Path;

use super::{SearchOptions, Source};
use crate::models::{Chapter, PageImage, SearchResult, SeriesChapters};
use crate::util::{
    USER_AGENT, absolute_url, get_text, id_from_href, normalize_whitespace, read_response_text,
    validate_id,
};

const BASE_URL: &str = "https://weebcentral.com";

pub struct WeebCentral;

impl Source for WeebCentral {
    fn id(&self) -> &'static str {
        "weebcentral"
    }

    fn name(&self) -> &'static str {
        "WeebCentral"
    }

    fn search(&self, keyword: &str, options: &SearchOptions) -> Result<Vec<SearchResult>, String> {
        search(keyword, options.cover_cache_dir, options.with_details)
    }

    fn series(&self, series_id: &str) -> Result<SeriesChapters, String> {
        load_chapters(series_id)
    }

    fn pages(&self, chapter: &Chapter) -> Result<Vec<PageImage>, String> {
        let html = get_text(&chapter.url)?;
        parse_pages(chapter, &html)
    }
}

fn abs(value: &str) -> String {
    absolute_url(BASE_URL, value)
}

const SEARCH_URL: &str = "https://weebcentral.com/search/simple?location=main";

/// Number of worker threads used to fetch covers and series details.
const SEARCH_WORKERS: usize = 4;

#[derive(Default)]
struct SeriesMeta {
    authors: String,
    tags: String,
    manga_type: String,
    status: String,
    released: String,
}

fn parse_series_meta(document: &Html) -> SeriesMeta {
    SeriesMeta {
        authors: list_item_value(document, "Author(s)"),
        tags: list_item_value(document, "Tags(s)"),
        manga_type: list_item_value(document, "Type"),
        status: list_item_value(document, "Status"),
        released: list_item_value(document, "Released"),
    }
}

pub fn search(
    keyword: &str,
    cover_cache_dir: &str,
    with_details: bool,
) -> Result<Vec<SearchResult>, String> {
    let response = ureq::post(SEARCH_URL)
        .set("User-Agent", USER_AGENT)
        .send_form(&[("text", keyword)])
        .map_err(|error| error.to_string())?;
    let html = read_response_text(response)?;
    let document = Html::parse_document(&html);
    let result_selector = Selector::parse("section#quick-search-result a.join-item")
        .expect("valid search result selector");
    let title_selector = Selector::parse("div.flex-1").expect("valid title selector");
    let picture_selector = Selector::parse("picture").expect("valid picture selector");
    let webp_selector =
        Selector::parse("source[type='image/webp']").expect("valid cover source selector");
    let image_selector = Selector::parse("img").expect("valid image selector");
    if !cover_cache_dir.is_empty() {
        fs::create_dir_all(cover_cache_dir).map_err(|error| error.to_string())?;
    }

    let mut results = Vec::new();
    for item in document.select(&result_selector) {
        let href = item.value().attr("href").unwrap_or_default();
        let id = id_from_href(href, "/series/");
        let title = item
            .select(&title_selector)
            .next()
            .map(|element| normalize_whitespace(element.text().collect::<Vec<_>>().join(" ")))
            .unwrap_or_default();
        if id.is_empty() || title.is_empty() {
            continue;
        }

        let cover_url = item
            .select(&picture_selector)
            .next()
            .and_then(|picture| {
                picture
                    .select(&webp_selector)
                    .next()
                    .and_then(|source| source.value().attr("srcset"))
                    .or_else(|| {
                        picture
                            .select(&image_selector)
                            .next()
                            .and_then(|image| image.value().attr("src"))
                    })
            })
            .map(abs)
            .unwrap_or_default();

        results.push(SearchResult {
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
        });
    }

    enrich_results(&mut results, cover_cache_dir, with_details);
    Ok(results)
}

/// Downloads covers and (optionally) series metadata for every result,
/// spreading the work over a few threads so the search stays responsive.
/// Failures are never fatal: a result simply keeps empty fields.
fn enrich_results(results: &mut [SearchResult], cover_cache_dir: &str, with_details: bool) {
    if results.is_empty() {
        return;
    }
    let chunk_size = results.len().div_ceil(SEARCH_WORKERS).max(1);
    std::thread::scope(|scope| {
        for chunk in results.chunks_mut(chunk_size) {
            scope.spawn(move || {
                for result in chunk {
                    result.cover_path =
                        cache_cover(&result.id, &result.cover_url, Path::new(cover_cache_dir));
                    if !with_details {
                        continue;
                    }
                    if let Ok(page) = get_text(&result.url) {
                        let meta = parse_series_meta(&Html::parse_document(&page));
                        result.authors = meta.authors;
                        result.tags = meta.tags;
                        result.manga_type = meta.manga_type;
                        result.status = meta.status;
                        result.released = meta.released;
                    }
                }
            });
        }
    });
}

pub fn load_chapters(series_id: &str) -> Result<SeriesChapters, String> {
    validate_id(series_id)?;
    let series_url = format!("{BASE_URL}/series/{series_id}");
    let series_document = Html::parse_document(&get_text(&series_url)?);
    let title_selector = Selector::parse("h1").expect("valid series title selector");
    let title = series_document
        .select(&title_selector)
        .next()
        .map(|element| normalize_whitespace(element.text().collect::<Vec<_>>().join(" ")))
        .unwrap_or_default();
    let cover_selector = Selector::parse("picture").expect("valid series cover selector");
    let webp_selector =
        Selector::parse("source[type='image/webp']").expect("valid cover source selector");
    let image_selector = Selector::parse("img").expect("valid image selector");
    let cover_url = series_document
        .select(&cover_selector)
        .next()
        .and_then(|picture| {
            picture
                .select(&webp_selector)
                .next()
                .and_then(|source| source.value().attr("srcset"))
                .or_else(|| {
                    picture
                        .select(&image_selector)
                        .next()
                        .and_then(|image| image.value().attr("src"))
                })
        })
        .map(abs)
        .unwrap_or_default();
    let meta = parse_series_meta(&series_document);
    let description = list_item_value(&series_document, "Description");

    let chapters_url = format!("{series_url}/full-chapter-list");
    let response = ureq::get(&chapters_url)
        .set("User-Agent", USER_AGENT)
        .set("HX-Request", "true")
        .call()
        .map_err(|error| error.to_string())?;
    let chapters_document = Html::parse_document(&read_response_text(response)?);
    let chapter_selector =
        Selector::parse("a[href^='/chapters/']").expect("valid chapter link selector");
    let chapter_title_selector =
        Selector::parse("span.grow > span:first-child").expect("valid chapter title selector");
    let time_selector = Selector::parse("time").expect("valid published date selector");

    let mut chapters = Vec::new();
    for item in chapters_document.select(&chapter_selector) {
        let href = item.value().attr("href").unwrap_or_default();
        let id = id_from_href(href, "/chapters/");
        if id.is_empty() {
            continue;
        }
        let title = item
            .select(&chapter_title_selector)
            .next()
            .map(|element| normalize_whitespace(element.text().collect::<Vec<_>>().join(" ")))
            .unwrap_or_else(|| "Unknown chapter".to_string());
        let published_at = item
            .select(&time_selector)
            .next()
            .and_then(|time| time.value().attr("datetime"))
            .unwrap_or_default()
            .to_string();

        chapters.push(Chapter {
            id: id.to_string(),
            title,
            url: abs(href),
            published_at,
        });
    }
    chapters.reverse();

    Ok(SeriesChapters {
        id: series_id.to_string(),
        title,
        url: series_url,
        cover_url,
        authors: meta.authors,
        tags: meta.tags,
        manga_type: meta.manga_type,
        status: meta.status,
        released: meta.released,
        description,
        chapters,
    })
}

/// Page images of a chapter. The reader HTML only preloads page 1, so the
/// remaining URLs are derived from its numbering scheme ("...-001.png").
fn parse_pages(chapter: &Chapter, chapter_html: &str) -> Result<Vec<PageImage>, String> {
    let document = Html::parse_document(chapter_html);
    let selector = Selector::parse("link[rel='preload']").expect("valid first page image selector");
    let first_page = document
        .select(&selector)
        .next()
        .and_then(|link| link.value().attr("href"))
        .map(abs)
        .ok_or_else(|| "Could not find the first page image URL".to_string())?;
    let pattern = Regex::new(r"^(?P<prefix>.+-)(?P<number>\d+)(?P<extension>\.\w+)(?:\?.*)?$")
        .map_err(|error| error.to_string())?;
    let captures = pattern
        .captures(&first_page)
        .ok_or_else(|| format!("Unrecognized page image URL: {first_page}"))?;
    let prefix = &captures["prefix"];
    let width = captures["number"].len();
    let extension = &captures["extension"];
    let referer = format!("{BASE_URL}/chapters/{}", chapter.id);

    Ok((1..=page_count(chapter_html))
        .map(|number| PageImage {
            url: format!("{prefix}{number:0width$}{extension}"),
            referer: Some(referer.clone()),
            headers: Default::default(),
            context: None,
        })
        .collect())
}

fn page_count(chapter_html: &str) -> usize {
    let pattern = Regex::new(r"page === (\d+) \?").expect("valid page count pattern");
    pattern
        .captures_iter(chapter_html)
        .filter_map(|capture| capture[1].parse::<usize>().ok())
        .max()
        .unwrap_or(1)
}

fn list_item_value(document: &Html, label: &str) -> String {
    let item_selector = Selector::parse("li").expect("valid metadata list selector");
    let strong_selector = Selector::parse("strong").expect("valid metadata label selector");
    let link_selector = Selector::parse("a").expect("valid metadata link selector");
    let span_selector = Selector::parse("span").expect("valid metadata value selector");
    let paragraph_selector = Selector::parse("p").expect("valid description selector");

    let Some(item) = document.select(&item_selector).find(|item| {
        item.select(&strong_selector)
            .next()
            .map(|strong| normalize_whitespace(strong.text().collect::<Vec<_>>().join(" ")))
            .map(|text| text.trim_end_matches(':') == label.trim_end_matches(':'))
            .unwrap_or(false)
    }) else {
        return String::new();
    };

    if label == "Description" {
        return item
            .select(&paragraph_selector)
            .next()
            .map(|paragraph| normalize_whitespace(paragraph.text().collect::<Vec<_>>().join(" ")))
            .unwrap_or_default();
    }
    let links = item
        .select(&link_selector)
        .map(|link| normalize_whitespace(link.text().collect::<Vec<_>>().join(" ")))
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if !links.is_empty() {
        return links.join(", ");
    }
    item.select(&span_selector)
        .next()
        .map(|span| normalize_whitespace(span.text().collect::<Vec<_>>().join(" ")))
        .unwrap_or_default()
}

fn cache_cover(id: &str, cover_url: &str, cache_dir: &Path) -> String {
    if cover_url.is_empty() || cache_dir.as_os_str().is_empty() {
        return String::new();
    }
    let extension = cover_url
        .split('?')
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
    // Reuse a previously downloaded cover instead of fetching it again.
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
#[path = "../../tests/unit/weebcentral/mod.rs"]
mod tests;
