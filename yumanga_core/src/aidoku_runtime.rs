use std::collections::HashMap;
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use aidoku::{Chapter as AidokuChapter, FilterValue, Manga, Page, PageContent};
use aidoku_test_runner::libs::{DefaultValue, UserDefaults, WasmEnv};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use url::Url;
use wasmer::{FunctionEnv, Instance, Module, Store};
use zip::ZipArchive;

use crate::models::{Chapter, PageImage, SearchResult, SeriesChapters};
use crate::sources::{SearchOptions, Source};
use crate::util;

pub const CATALOG_URL: &str = "https://aidoku-community.github.io/sources/index.min.json";

#[unsafe(no_mangle)]
pub extern "C" fn destroy(_descriptor: i32) {}

#[derive(Debug, Deserialize)]
struct Catalog {
    name: String,
    sources: Vec<CatalogSource>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CatalogSource {
    id: String,
    name: String,
    version: u32,
    #[serde(rename = "downloadURL")]
    download_url: String,
    #[serde(default)]
    languages: Vec<String>,
    #[serde(default)]
    content_rating: Option<u8>,
}

#[derive(Deserialize)]
struct AidokuSearchPage {
    entries: Vec<Manga>,
    #[serde(rename = "has_next_page")]
    _has_next_page: bool,
}

struct ResolvedImageRequest {
    url: Option<String>,
    headers: HashMap<String, String>,
}

struct SourcePackage {
    wasm: Vec<u8>,
    defaults: UserDefaults,
}

fn resolve_image_request_descriptor(
    environment: &mut WasmEnv,
    bytes: &[u8],
) -> Result<ResolvedImageRequest, String> {
    let descriptor: i32 = postcard::from_bytes(bytes)
        .map_err(|error| format!("Could not decode Aidoku image request descriptor: {error}"))?;
    let request = environment
        .store
        .get_mut(descriptor)
        .and_then(|item| item.as_request())
        .ok_or_else(|| "Aidoku image request descriptor is invalid".to_string())?;
    let headers = request
        .headers
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_string(), value.to_string()))
        })
        .collect();
    Ok(ResolvedImageRequest {
        url: request.url.as_ref().map(ToString::to_string),
        headers,
    })
}

pub fn catalog_sources() -> Result<Vec<CatalogSourceInfo>, String> {
    let catalog = load_catalog()?;
    let _catalog_name = catalog.name;
    Ok(catalog
        .sources
        .into_iter()
        .map(|source| CatalogSourceInfo {
            id: source.id,
            name: source.name,
            languages: source.languages,
            content_rating: source.content_rating,
        })
        .collect())
}

pub struct CatalogSourceInfo {
    pub id: String,
    pub name: String,
    pub languages: Vec<String>,
    pub content_rating: Option<u8>,
}

pub fn find_source(source_id: &str) -> Result<Option<Box<dyn Source>>, String> {
    let catalog = load_catalog()?;
    let Some(source) = catalog
        .sources
        .into_iter()
        .find(|source| source.id == source_id)
    else {
        return Ok(None);
    };
    let package = load_source_package(&source)?;
    Ok(Some(Box::new(AidokuSource {
        id: source.id,
        name: source.name,
        wasm: package.wasm,
        defaults: package.defaults,
        manga_by_chapter: Mutex::new(HashMap::new()),
    })))
}

fn load_catalog() -> Result<Catalog, String> {
    let cache_path = cache_directory().join("index.min.json");
    match util::get_text(CATALOG_URL) {
        Ok(response) => {
            let catalog: Catalog = serde_json::from_str(&response)
                .map_err(|error| format!("Could not parse Aidoku source catalog: {error}"))?;
            if let Some(parent) = cache_path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(cache_path, response);
            Ok(catalog)
        }
        Err(network_error) => fs::read_to_string(&cache_path)
            .map_err(|_| format!("Could not fetch Aidoku catalog: {network_error}"))
            .and_then(|response| {
                serde_json::from_str(&response)
                    .map_err(|error| format!("Could not parse cached Aidoku catalog: {error}"))
            }),
    }
}

fn cache_directory() -> PathBuf {
    std::env::var_os("YUMANGA_AIDOKU_CACHE_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("XDG_CACHE_HOME")
                .map(PathBuf::from)
                .map(|directory| directory.join("yumanga/aidoku"))
        })
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|directory| directory.join(".cache/yumanga/aidoku"))
        })
        .unwrap_or_else(|| std::env::temp_dir().join("yumanga-aidoku-cache"))
}

fn load_source_package(source: &CatalogSource) -> Result<SourcePackage, String> {
    if source.id.is_empty()
        || source.id.contains("..")
        || !source
            .id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '-'))
    {
        return Err("Aidoku catalog contains an invalid source ID".to_string());
    }
    let directory = cache_directory();
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let wasm_path = directory.join(format!("{}-v{}.wasm", source.id, source.version));
    let settings_path = directory.join(format!("{}-v{}.settings.json", source.id, source.version));
    if wasm_path.is_file() && settings_path.is_file() {
        let wasm = fs::read(wasm_path).map_err(|error| error.to_string())?;
        let settings = fs::read_to_string(settings_path).map_err(|error| error.to_string())?;
        return Ok(SourcePackage {
            wasm,
            defaults: source_defaults_from_settings(&settings, &source.languages)?,
        });
    }

    let catalog_url = Url::parse(CATALOG_URL).map_err(|error| error.to_string())?;
    let package_url = catalog_url
        .join(&source.download_url)
        .map_err(|error| format!("Invalid Aidoku package URL: {error}"))?;
    let response = ureq::get(package_url.as_str())
        .set("User-Agent", util::USER_AGENT)
        .call()
        .map_err(|error| format!("Could not download {}: {error}", source.name))?;
    let mut package = Vec::new();
    response
        .into_reader()
        .take(64 * 1024 * 1024)
        .read_to_end(&mut package)
        .map_err(|error| error.to_string())?;
    if package.len() >= 64 * 1024 * 1024 {
        return Err("Aidoku source package exceeds the 64 MiB download limit".to_string());
    }

    let mut archive = ZipArchive::new(Cursor::new(package))
        .map_err(|error| format!("Invalid Aidoku package: {error}"))?;
    let mut wasm = Vec::new();
    {
        let wasm_file = archive
            .by_name("Payload/main.wasm")
            .map_err(|error| format!("Aidoku package has no Payload/main.wasm: {error}"))?;
        wasm_file
            .take(32 * 1024 * 1024)
            .read_to_end(&mut wasm)
            .map_err(|error| error.to_string())?;
    }
    if wasm.len() >= 32 * 1024 * 1024 {
        return Err("Aidoku WebAssembly module exceeds the 32 MiB limit".to_string());
    }
    let mut settings = Vec::new();
    if let Ok(settings_file) = archive.by_name("Payload/settings.json") {
        settings_file
            .take(1024 * 1024)
            .read_to_end(&mut settings)
            .map_err(|error| error.to_string())?;
    } else {
        settings.extend_from_slice(b"[]");
    }
    if settings.len() >= 1024 * 1024 {
        return Err("Aidoku source settings exceed the 1 MiB limit".to_string());
    }
    let settings = String::from_utf8(settings)
        .map_err(|error| format!("Aidoku source settings are not UTF-8: {error}"))?;
    let defaults = source_defaults_from_settings(&settings, &source.languages)?;

    let temporary_path = wasm_path.with_extension("wasm.part");
    fs::write(&temporary_path, &wasm).map_err(|error| error.to_string())?;
    fs::rename(temporary_path, wasm_path).map_err(|error| error.to_string())?;
    let temporary_settings_path = settings_path.with_extension("settings.json.part");
    fs::write(&temporary_settings_path, &settings).map_err(|error| error.to_string())?;
    fs::rename(temporary_settings_path, settings_path).map_err(|error| error.to_string())?;
    Ok(SourcePackage { wasm, defaults })
}

fn source_defaults_from_settings(
    settings: &str,
    supported_languages: &[String],
) -> Result<UserDefaults, String> {
    let settings: Value = serde_json::from_str(settings)
        .map_err(|error| format!("Could not parse Aidoku source settings: {error}"))?;
    let mut defaults = UserDefaults::new();
    collect_source_defaults(&settings, &mut defaults);
    if defaults.get("languages").is_none() {
        let languages = supported_languages
            .iter()
            .filter(|language| !matches!(language.as_str(), "All" | "multi"))
            .cloned()
            .collect::<Vec<_>>();
        defaults.set(
            "languages".to_string(),
            DefaultValue::StringArray(languages),
        );
    }
    Ok(defaults)
}

fn collect_source_defaults(value: &Value, defaults: &mut UserDefaults) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_source_defaults(item, defaults);
            }
        }
        Value::Object(object) => {
            if let (Some(key), Some(value)) = (
                object.get("key").and_then(Value::as_str),
                object.get("default"),
            ) {
                if let Some(value) = source_default_value(value) {
                    defaults.set(key.to_string(), value);
                }
            }
            if let Some(items) = object.get("items") {
                collect_source_defaults(items, defaults);
            }
        }
        _ => {}
    }
}

fn source_default_value(value: &Value) -> Option<DefaultValue> {
    match value {
        Value::Null => Some(DefaultValue::Null),
        Value::Bool(value) => Some(DefaultValue::Bool(*value)),
        Value::Number(value) => value
            .as_i64()
            .and_then(|value| i32::try_from(value).ok())
            .map(DefaultValue::Int)
            .or_else(|| {
                value
                    .as_f64()
                    .map(|value| DefaultValue::Float(value as f32))
            }),
        Value::String(value) => Some(DefaultValue::String(value.clone())),
        Value::Array(values) => values
            .iter()
            .map(Value::as_str)
            .collect::<Option<Vec<_>>>()
            .map(|values| {
                DefaultValue::StringArray(values.into_iter().map(str::to_string).collect())
            }),
        Value::Object(values) => values
            .iter()
            .map(|(key, value)| value.as_str().map(|value| (key.clone(), value.to_string())))
            .collect::<Option<HashMap<_, _>>>()
            .map(DefaultValue::HashMap),
    }
}

struct AidokuSource {
    id: String,
    name: String,
    wasm: Vec<u8>,
    defaults: UserDefaults,
    manga_by_chapter: Mutex<HashMap<String, Manga>>,
}

impl AidokuSource {
    fn has_export(&self, export_name: &str) -> Result<bool, String> {
        let store = Store::default();
        let module = Module::new(&store, self.wasm.as_slice())
            .map_err(|error| format!("Could not load Aidoku source: {error}"))?;
        Ok(module.exports().any(|export| export.name() == export_name))
    }

    fn invoke<T, F>(&self, operation: &str, call: F) -> Result<T, String>
    where
        T: DeserializeOwned,
        F: FnOnce(&mut Store, &Instance, &FunctionEnv<WasmEnv>) -> Result<i32, String>,
    {
        self.invoke_with(call, |bytes, _store, _environment| {
            postcard::from_bytes(bytes)
                .map_err(|error| format!("Could not decode Aidoku {operation} result: {error}"))
        })
    }

    fn invoke_with<T, F, D>(&self, call: F, decode: D) -> Result<T, String>
    where
        F: FnOnce(&mut Store, &Instance, &FunctionEnv<WasmEnv>) -> Result<i32, String>,
        D: FnOnce(&[u8], &mut Store, &FunctionEnv<WasmEnv>) -> Result<T, String>,
    {
        let mut store = Store::default();
        let module = Module::new(&store, self.wasm.as_slice())
            .map_err(|error| format!("Could not load Aidoku source: {error}"))?;
        let mut wasm_environment = WasmEnv::new();
        wasm_environment.defaults = self.defaults.clone();
        let environment = FunctionEnv::new(&mut store, wasm_environment);
        let imports = aidoku_test_runner::imports::generate_imports(&mut store, &environment);
        let instance = Instance::new(&mut store, &module, &imports)
            .map_err(|error| format!("Could not initialize Aidoku source: {error}"))?;
        {
            let memory = instance
                .exports
                .get_memory("memory")
                .map_err(|error| format!("Aidoku source has no linear memory: {error}"))?
                .clone();
            environment.as_mut(&mut store).memory = Some(memory);
        }

        let start = instance
            .exports
            .get_typed_function::<(), ()>(&store, "start")
            .map_err(|error| format!("Aidoku source has no start function: {error}"))?;
        start
            .call(&mut store)
            .map_err(|error| format!("Aidoku source initialization failed: {error}"))?;

        let result_pointer = call(&mut store, &instance, &environment)?;
        if result_pointer <= 0 {
            let message = environment.as_ref(&store).stdout.trim();
            return Err(if message.is_empty() {
                format!("Aidoku source returned error {result_pointer}")
            } else {
                message.to_string()
            });
        }

        let wasm_environment = environment.as_ref(&store);
        let result_length = wasm_environment
            .read_u32(&store, result_pointer as u32)
            .unwrap_or_default();
        if result_length > 64 * 1024 * 1024 {
            let error_length = wasm_environment
                .read_u32(&store, result_pointer as u32 + 8)
                .map_err(|error| format!("Could not read Aidoku error length: {error}"))?
                .saturating_sub(12);
            let error_message = wasm_environment
                .read_bytes(&store, result_pointer as u32 + 12, error_length)
                .map_err(|error| format!("Could not read Aidoku error message: {error}"))?;
            if let Ok(free_result) = instance
                .exports
                .get_typed_function::<i32, ()>(&store, "free_result")
            {
                let _ = free_result.call(&mut store, result_pointer);
            }
            let message = String::from_utf8_lossy(&error_message)
                .chars()
                .take_while(|character| !character.is_control())
                .collect::<String>()
                .trim()
                .to_string();
            let error = if message.is_empty() {
                "Aidoku source returned an unspecified error".to_string()
            } else {
                message
            };
            let diagnostic = environment.as_ref(&store).stdout.trim();
            return Err(if diagnostic.is_empty() {
                error
            } else {
                format!("{error}\nAidoku runtime log:\n{diagnostic}")
            });
        }
        let bytes = wasm_environment
            .read_item_bytes(&store, result_pointer as u32)
            .map_err(|error| {
                format!(
                    "Could not read Aidoku result at pointer {result_pointer} (length {result_length}): {error}"
                )
            })?;
        let result = decode(&bytes, &mut store, &environment).map_err(|error| {
            let diagnostic = environment.as_ref(&store).stdout.trim();
            if diagnostic.is_empty() {
                error
            } else {
                format!("{error}\nAidoku runtime log:\n{diagnostic}")
            }
        });
        if let Ok(free_result) = instance
            .exports
            .get_typed_function::<i32, ()>(&store, "free_result")
        {
            let _ = free_result.call(&mut store, result_pointer);
        }
        result
    }

    fn update_manga(&self, manga: Manga, details: bool, chapters: bool) -> Result<Manga, String> {
        self.invoke("get_manga_update", |store, instance, environment| {
            let descriptor = environment
                .as_mut(store)
                .store
                .store_encoded(&manga)
                .map_err(|error| error.to_string())?;
            let update = instance
                .exports
                .get_typed_function::<(i32, i32, i32), i32>(store, "get_manga_update")
                .map_err(|error| error.to_string())?;
            update
                .call(store, descriptor, i32::from(details), i32::from(chapters))
                .map_err(|error| error.to_string())
        })
    }

    fn series_manga(&self, series_id: &str) -> Result<Manga, String> {
        self.update_manga(
            Manga {
                key: series_id.to_string(),
                ..Manga::default()
            },
            true,
            true,
        )
    }

    fn map_series(&self, manga: Manga) -> Result<SeriesChapters, String> {
        let mut chapters = manga.chapters.clone().unwrap_or_default();
        chapters.sort_by(
            |left, right| match (left.chapter_number, right.chapter_number) {
                (Some(left), Some(right)) => left.total_cmp(&right),
                _ => left.date_uploaded.cmp(&right.date_uploaded),
            },
        );
        let mapped = chapters
            .iter()
            .map(|chapter| Chapter {
                id: chapter.key.clone(),
                title: chapter
                    .title
                    .clone()
                    .or_else(|| {
                        chapter
                            .chapter_number
                            .map(|number| format!("Chapter {number}"))
                    })
                    .unwrap_or_else(|| chapter.key.clone()),
                url: chapter.url.clone().unwrap_or_default(),
                published_at: chapter
                    .date_uploaded
                    .map(|date| date.to_string())
                    .unwrap_or_default(),
            })
            .collect::<Vec<_>>();
        let mut manga_by_chapter = self
            .manga_by_chapter
            .lock()
            .map_err(|_| "Aidoku source state lock was poisoned".to_string())?;
        for chapter in &mapped {
            manga_by_chapter.insert(chapter.id.clone(), manga.clone());
        }
        Ok(SeriesChapters {
            id: manga.key,
            title: manga.title,
            url: manga.url.unwrap_or_default(),
            cover_url: manga.cover.unwrap_or_default(),
            authors: manga.authors.unwrap_or_default().join(", "),
            tags: manga.tags.unwrap_or_default().join(", "),
            manga_type: String::new(),
            status: format!("{:?}", manga.status),
            released: String::new(),
            description: manga.description.unwrap_or_default(),
            chapters: mapped,
        })
    }
}

impl Source for AidokuSource {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn search_filters(&self) -> &'static [&'static str] {
        &[]
    }

    fn search(&self, keyword: &str, options: &SearchOptions) -> Result<Vec<SearchResult>, String> {
        let query = (!keyword.trim().is_empty()).then(|| keyword.to_string());
        let mut entries: AidokuSearchPage =
            self.invoke("get_search_manga_list", |store, instance, environment| {
                let query_descriptor = environment
                    .as_mut(store)
                    .store
                    .store_encoded(&query)
                    .map_err(|error| error.to_string())?;
                let filters: Vec<FilterValue> = Vec::new();
                let filters_descriptor = environment
                    .as_mut(store)
                    .store
                    .store_encoded(&filters)
                    .map_err(|error| error.to_string())?;
                let search = instance
                    .exports
                    .get_typed_function::<(i32, i32, i32), i32>(store, "get_search_manga_list")
                    .map_err(|error| error.to_string())?;
                search
                    .call(store, query_descriptor, 1, filters_descriptor)
                    .map_err(|error| error.to_string())
            })?;

        if options.with_details {
            for manga in &mut entries.entries {
                if let Ok(updated) = self.update_manga(manga.clone(), true, false) {
                    *manga = updated;
                }
            }
        }

        let mut results = entries
            .entries
            .into_iter()
            .map(|manga| SearchResult {
                id: manga.key,
                title: manga.title,
                url: manga.url.unwrap_or_default(),
                cover_url: manga.cover.unwrap_or_default(),
                cover_path: String::new(),
                authors: manga.authors.unwrap_or_default().join(", "),
                tags: manga.tags.unwrap_or_default().join(", "),
                manga_type: String::new(),
                status: format!("{:?}", manga.status),
                released: String::new(),
            })
            .collect::<Vec<_>>();
        cache_covers(&mut results, options.cover_cache_dir);
        Ok(results)
    }

    fn series(&self, series_id: &str) -> Result<SeriesChapters, String> {
        self.map_series(self.series_manga(series_id)?)
    }

    fn pages(&self, chapter: &Chapter) -> Result<Vec<PageImage>, String> {
        let manga = self
            .manga_by_chapter
            .lock()
            .map_err(|_| "Aidoku source state lock was poisoned".to_string())?
            .get(&chapter.id)
            .cloned()
            .ok_or_else(|| "Load the series before requesting chapter pages".to_string())?;
        let aidoku_chapter: AidokuChapter = manga
            .chapters
            .as_ref()
            .and_then(|chapters| chapters.iter().find(|item| item.key == chapter.id))
            .cloned()
            .ok_or_else(|| "Chapter is not present in the Aidoku series".to_string())?;
        let pages: Vec<Page> = self.invoke("get_page_list", |store, instance, environment| {
            let manga_descriptor = environment
                .as_mut(store)
                .store
                .store_encoded(&manga)
                .map_err(|error| error.to_string())?;
            let chapter_descriptor = environment
                .as_mut(store)
                .store
                .store_encoded(&aidoku_chapter)
                .map_err(|error| error.to_string())?;
            let get_pages = instance
                .exports
                .get_typed_function::<(i32, i32), i32>(store, "get_page_list")
                .map_err(|error| error.to_string())?;
            get_pages
                .call(store, manga_descriptor, chapter_descriptor)
                .map_err(|error| error.to_string())
        })?;

        pages
            .into_iter()
            .map(|page| match page.content {
                PageContent::Url(url, context) => Ok(PageImage {
                    url,
                    referer: manga.url.clone(),
                    headers: HashMap::new(),
                    context: context.map(|context| context.into_iter().collect()),
                }),
                PageContent::Zip(url, _) => Err(format!(
                    "Aidoku page archives are not supported yet: {url}"
                )),
                PageContent::Text(_) => Err("Aidoku source returned a text page, not an image".to_string()),
                PageContent::Image(_) => Err("Aidoku source returned an internally processed image, which is not supported yet".to_string()),
            })
            .collect()
    }

    fn fetch_image(&self, page: &PageImage) -> Result<Vec<u8>, String> {
        if !self.has_export("get_image_request")? {
            return util::get_bytes(&page.url, page.referer.as_deref());
        }
        let request = self.invoke_with(
            |store, instance, environment| {
                let url_descriptor = environment
                    .as_mut(store)
                    .store
                    .store_encoded(&page.url)
                    .map_err(|error| error.to_string())?;
                let context_descriptor = match &page.context {
                    Some(context) => environment
                        .as_mut(store)
                        .store
                        .store_encoded(context)
                        .map_err(|error| error.to_string())?,
                    None => -1,
                };
                let get_image_request = instance
                    .exports
                    .get_typed_function::<(i32, i32), i32>(store, "get_image_request")
                    .map_err(|error| error.to_string())?;
                get_image_request
                    .call(store, url_descriptor, context_descriptor)
                    .map_err(|error| error.to_string())
            },
            |bytes, store, environment| {
                resolve_image_request_descriptor(environment.as_mut(store), bytes)
            },
        )?;
        let url = request.url.as_deref().unwrap_or(&page.url);
        let mut headers = page.headers.clone();
        headers.extend(request.headers.into_iter());
        util::get_bytes_with_headers(url, page.referer.as_deref(), &headers)
    }
}

fn cache_covers(results: &mut [SearchResult], cache_directory: &str) {
    if cache_directory.is_empty() || fs::create_dir_all(cache_directory).is_err() {
        return;
    }
    for result in results {
        let extension = util::image_extension(&result.cover_url);
        let path =
            Path::new(cache_directory).join(format!("{}{}", safe_cover_id(&result.id), extension));
        if path.is_file() || result.cover_url.is_empty() {
            result.cover_path = path.to_string_lossy().into_owned();
            continue;
        }
        if let Ok(bytes) = util::get_bytes(&result.cover_url, None) {
            if fs::write(&path, bytes).is_ok() {
                result.cover_path = path.to_string_lossy().into_owned();
            }
        }
    }
}

fn safe_cover_id(id: &str) -> String {
    id.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        Catalog, CatalogSource, resolve_image_request_descriptor, source_defaults_from_settings,
    };
    use aidoku_test_runner::libs::{DefaultValue, HttpMethod, NetRequest, StoreItem, WasmEnv};
    use url::Url;

    #[test]
    fn parses_community_index_entries() {
        let index = r#"{"name":"Aidoku Community Sources","sources":[{"id":"en.example","name":"Example","version":7,"iconURL":"icons/example.png","downloadURL":"sources/example.aix","languages":["en"],"contentRating":1,"baseURL":"https://example.org"}]}"#;
        let catalog: Catalog = serde_json::from_str(index).expect("catalog should parse");

        assert_eq!(catalog.name, "Aidoku Community Sources");
        assert_eq!(catalog.sources.len(), 1);
        assert_eq!(catalog.sources[0].id, "en.example");
        assert_eq!(catalog.sources[0].download_url, "sources/example.aix");
        assert_eq!(catalog.sources[0].languages, ["en"]);
        assert_eq!(catalog.sources[0].content_rating, Some(1));
    }

    #[test]
    fn rejects_catalog_entries_without_download_url() {
        let source = r#"{"id":"en.example","name":"Example","version":1}"#;
        assert!(serde_json::from_str::<CatalogSource>(source).is_err());
    }

    #[test]
    fn defaults_missing_source_filter_metadata() {
        let source = r#"{"id":"en.example","name":"Example","version":1,"downloadURL":"sources/example.aix"}"#;
        let source: CatalogSource = serde_json::from_str(source).expect("source should parse");

        assert!(source.languages.is_empty());
        assert_eq!(source.content_rating, None);
    }

    #[test]
    fn loads_package_defaults_and_falls_back_to_supported_languages() {
        let defaults = source_defaults_from_settings(
            r#"[{"type":"group","items":[{"key":"contentRating","default":["safe","suggestive"]}]}]"#,
            &[
                "en".to_string(),
                "id".to_string(),
                "All".to_string(),
                "multi".to_string(),
            ],
        )
        .expect("source settings should parse");

        assert!(matches!(
            defaults.get("contentRating"),
            Some(DefaultValue::StringArray(values)) if values == &["safe", "suggestive"]
        ));
        assert!(matches!(
            defaults.get("languages"),
            Some(DefaultValue::StringArray(values)) if values == &["en", "id"]
        ));
    }

    #[test]
    fn preserves_language_defaults_when_the_package_defines_them() {
        let defaults = source_defaults_from_settings(
            r#"[{"key":"languages","default":["id","en"]}]"#,
            &["en".to_string(), "id".to_string()],
        )
        .expect("source settings should parse");

        assert!(matches!(
            defaults.get("languages"),
            Some(DefaultValue::StringArray(values)) if values == &["id", "en"]
        ));
    }

    #[test]
    fn resolves_image_request_result_as_host_descriptor() {
        let mut environment = WasmEnv::new();
        let mut request = NetRequest::new(HttpMethod::Get);
        request.url = Some(Url::parse("https://images.example/page.webp").unwrap());
        let descriptor = environment
            .store
            .store(StoreItem::Request(Box::new(request)));
        let bytes = postcard::to_allocvec(&descriptor).unwrap();

        let image_request = resolve_image_request_descriptor(&mut environment, &bytes).unwrap();

        assert_eq!(
            image_request.url.as_deref(),
            Some("https://images.example/page.webp")
        );
        assert!(image_request.headers.is_empty());
    }
}
