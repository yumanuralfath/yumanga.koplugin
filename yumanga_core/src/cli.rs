use serde::Serialize;
use serde_json::json;
use std::env;
use std::process::ExitCode;

use crate::download;
use crate::models::SourceInfo;
use crate::sources::{self, SearchOptions, Source};

const USAGE: &str = "Usage:
  core sources
    core search   <source> <keyword> [cover-cache-dir] [with-details: 0|1] [genre]
  core chapters <source> <series-id>
  core download <source> <series-id> <selection> <volume-name> <output-dir> <progress-file>";

pub fn run() -> ExitCode {
    let arguments: Vec<String> = env::args().skip(1).collect();
    let argument = |index: usize| arguments.get(index).map(String::as_str);

    match argument(0) {
        Some("sources") => list_sources(),
        Some("search") => with_source(argument(1), |source| match argument(2) {
            Some(keyword)
                if !keyword.trim().is_empty()
                    || !argument(5).unwrap_or_default().trim().is_empty() =>
            {
                let genre = argument(5).unwrap_or_default();
                let mut results = source.search(
                    keyword,
                    &SearchOptions {
                        cover_cache_dir: argument(3).unwrap_or_default(),
                        // Genre filtering requires source metadata even when details are hidden.
                        with_details: argument(4) != Some("0") || !genre.trim().is_empty(),
                        genre,
                    },
                )?;
                if source.search_filters().contains(&"genre") {
                    sources::filter_results_by_genre(&mut results, genre);
                }
                Ok(results)
            }
            _ => Err("Search keyword is required".to_string()),
        }),
        Some("chapters") => with_source(argument(1), |source| match argument(2) {
            Some(series_id) => source.series(series_id),
            None => Err("Series ID is required".to_string()),
        }),
        Some("download") => with_source(argument(1), |source| {
            match (
                argument(2),
                argument(3),
                argument(4),
                argument(5),
                argument(6),
            ) {
                (
                    Some(series_id),
                    Some(selection),
                    Some(volume_name),
                    Some(output_dir),
                    Some(progress_path),
                ) => download::download_selection(
                    source,
                    series_id,
                    selection,
                    volume_name,
                    output_dir,
                    progress_path,
                ),
                _ => Err(USAGE.to_string()),
            }
        }),
        _ => emit_error(USAGE),
    }
}

fn with_source<T: Serialize>(
    source_id: Option<&str>,
    action: impl FnOnce(&dyn Source) -> Result<T, String>,
) -> ExitCode {
    let Some(source_id) = source_id else {
        return emit_error("Source ID is required");
    };
    match find_source(source_id) {
        Ok(Some(source)) => emit_result(action(source.as_ref())),
        Ok(None) => emit_error(format!("Unknown source: {source_id}")),
        Err(error) => emit_error(error),
    }
}

fn list_sources() -> ExitCode {
    #[allow(unused_mut)]
    let mut available = sources::all()
        .iter()
        .map(|source| SourceInfo {
            id: source.id().to_string(),
            name: source.name().to_string(),
            search_filters: source
                .search_filters()
                .iter()
                .map(|filter| filter.to_string())
                .collect(),
        })
        .collect::<Vec<_>>();

    #[cfg(feature = "aidoku-runtime")]
    match crate::aidoku_runtime::catalog_sources() {
        Ok(catalog) => available.extend(catalog.into_iter().map(|source| SourceInfo {
            id: source.id,
            name: source.name,
            search_filters: Vec::new(),
        })),
        Err(error) => return emit_error(format!("Could not load Aidoku catalog: {error}")),
    }

    emit_result(Ok(available))
}

fn find_source(source_id: &str) -> Result<Option<Box<dyn Source>>, String> {
    if let Some(source) = sources::find(source_id) {
        return Ok(Some(source));
    }

    #[cfg(feature = "aidoku-runtime")]
    return crate::aidoku_runtime::find_source(source_id);

    #[cfg(not(feature = "aidoku-runtime"))]
    Ok(None)
}

fn emit_result<T: Serialize>(result: Result<T, String>) -> ExitCode {
    match result {
        Ok(data) => {
            println!("{}", json!({ "ok": true, "data": data }));
            ExitCode::SUCCESS
        }
        Err(error) => emit_error(error),
    }
}

fn emit_error(error: impl std::fmt::Display) -> ExitCode {
    println!("{}", json!({ "ok": false, "error": error.to_string() }));
    ExitCode::FAILURE
}
