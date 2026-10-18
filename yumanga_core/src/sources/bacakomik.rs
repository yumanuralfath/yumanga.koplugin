//! bacakomik.my
use scraper::{Html, Selector};
use std::fs;
use std::path::Path;

use super::{SearchOptions, Source};
use crate::models::{Chapter, PageImage, SearchResult, SeriesChapters};
use crate::util::{
    USER_AGENT, absolute_url, get_text, id_from_href, normalize_whitespace, read_response_text,
    validate_id,
};

const BASE_URL: &str = "https://bacakomik.my";

pub struct BacaKomik;

impl Source for BacaKomik {
    fn id(&self) -> &'static str {
        "bacakomik"
    }

    fn name(&self) -> &'static str {
        "BacaKomik"
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
    let response = ureq::get(BASE_URL)
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
    let result_selector = Selector::parse(".film-list .animepost .animposx > a[itemprop='url']")
        .expect("valid search result selector");
    let image_selector = Selector::parse("img").expect("valid search cover selector");
    document
        .select(&result_selector)
        .filter_map(|item| {
            let href = item.value().attr("href").unwrap_or_default();
            let id = id_from_href(href, "/komik/");
            let title = item
                .value()
                .attr("title")
                .or_else(|| item.value().attr("alt"))
                .map(|title| title.trim_start_matches("Komik ").to_string())
                .filter(|title| !title.is_empty())
                .or_else(|| {
                    item.select(&image_selector)
                        .next()
                        .and_then(|image| image.value().attr("alt"))
                        .map(|title| title.trim_start_matches("Komik ").to_string())
                })
                .unwrap_or_default();
            if id.is_empty() || title.is_empty() {
                return None;
            }
            let cover_url = item
                .select(&image_selector)
                .next()
                .and_then(|image| {
                    image
                        .value()
                        .attr("data-lazy-src")
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
    validate_id(series_id)?;
    let url = format!("{BASE_URL}/komik/{series_id}/");
    let document = Html::parse_document(&get_text(&url)?);
    parse_series(series_id, &url, &document)
}

fn parse_series(series_id: &str, url: &str, document: &Html) -> Result<SeriesChapters, String> {
    let title_selector = Selector::parse("h1.entry-title").expect("valid series title selector");
    let title = document
        .select(&title_selector)
        .next()
        .map(|element| normalize_whitespace(element.text().collect::<Vec<_>>().join(" ")))
        .unwrap_or_default()
        .trim_start_matches("Komik ")
        .to_string();
    let cover_selector = Selector::parse(".thumb img").expect("valid series cover selector");
    let cover_url = document
        .select(&cover_selector)
        .next()
        .and_then(|image| {
            image
                .value()
                .attr("data-lazy-src")
                .or_else(|| image.value().attr("src"))
        })
        .map(abs)
        .unwrap_or_default();
    let description_selector = Selector::parse(".shortcsc").expect("valid description selector");
    let description = document
        .select(&description_selector)
        .next()
        .map(|element| normalize_whitespace(element.text().collect::<Vec<_>>().join(" ")))
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
        document,
    );
    let chapters = parse_chapters(document);

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
    let info_selector = Selector::parse(".infox .spe span").expect("valid metadata selector");
    let label_selector = Selector::parse("b").expect("valid metadata label selector");
    let link_selector = Selector::parse("a").expect("valid metadata link selector");
    for item in document.select(&info_selector) {
        let Some(label) = item.select(&label_selector).next() else {
            continue;
        };
        let label = normalize_whitespace(label.text().collect::<Vec<_>>().join(" "))
            .trim_end_matches(':')
            .to_ascii_lowercase();
        let links = item
            .select(&link_selector)
            .map(|link| normalize_whitespace(link.text().collect::<Vec<_>>().join(" ")))
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
        let value = if links.is_empty() {
            normalize_whitespace(item.text().collect::<Vec<_>>().join(" "))
                .split_once(':')
                .map(|(_, value)| value.trim().to_string())
                .unwrap_or_default()
        } else {
            links.join(", ")
        };
        match label.as_str() {
            "author" | "artis" | "pengarang" => append_value(authors, value),
            "jenis komik" | "tipe" | "type" => *manga_type = value,
            "status" => *status = value,
            "rilis" | "tahun" | "released" => *released = value,
            _ => {}
        }
    }
    let genre_selector = Selector::parse(".genre-info a").expect("valid genre selector");
    let genres = document
        .select(&genre_selector)
        .map(|genre| normalize_whitespace(genre.text().collect::<Vec<_>>().join(" ")))
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>();
    if !genres.is_empty() {
        *tags = genres.join(", ");
    }
}

fn append_value(target: &mut String, value: String) {
    if value.is_empty() || target.split(", ").any(|existing| existing == value) {
        return;
    }
    if !target.is_empty() {
        target.push_str(", ");
    }
    target.push_str(&value);
}

fn parse_chapters(document: &Html) -> Vec<Chapter> {
    let row_selector = Selector::parse("#chapter_list li").expect("valid chapter row selector");
    let link_selector = Selector::parse(".lchx a").expect("valid chapter link selector");
    let number_selector = Selector::parse("chapter").expect("valid chapter number selector");
    let date_selector = Selector::parse(".dt").expect("valid chapter date selector");
    let mut chapters = Vec::new();
    for row in document.select(&row_selector) {
        let Some(link) = row.select(&link_selector).next() else {
            continue;
        };
        let href = link.value().attr("href").unwrap_or_default();
        let Some(id) = slug_from_href(href, "/") else {
            continue;
        };
        let chapter_number = link
            .select(&number_selector)
            .next()
            .map(|number| normalize_whitespace(number.text().collect::<Vec<_>>().join(" ")))
            .filter(|number| !number.is_empty());
        let title = chapter_number
            .map(|number| format!("Chapter {number}"))
            .unwrap_or_else(|| normalize_whitespace(link.text().collect::<Vec<_>>().join(" ")));
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
    let image_selector =
        Selector::parse("img[data-lazy-src*='/media/']").expect("valid chapter image selector");
    let pages = document
        .select(&image_selector)
        .filter_map(|image| {
            image
                .value()
                .attr("data-lazy-src")
                .or_else(|| image.value().attr("src"))
        })
        .filter(|url| !url.is_empty())
        .map(|url| PageImage {
            url: url.to_string(),
            referer: Some(chapter.url.clone()),
            headers: Default::default(),
            context: None,
        })
        .collect::<Vec<_>>();
    if pages.is_empty() {
        return Err("Could not find BacaKomik chapter page images".to_string());
    }
    Ok(pages)
}

fn slug_from_href<'a>(href: &'a str, route: &str) -> Option<&'a str> {
    let path = href.strip_prefix(BASE_URL).unwrap_or(href);
    let path = path.strip_prefix(route)?;
    let slug = path.split(['?', '#']).next()?.trim_matches('/');
    (!slug.is_empty()).then_some(slug)
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
#[path = "../../tests/unit/bacakomik/mod.rs"]
mod tests;
