use std::io::Read;

pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36";

pub fn get_text(url: &str) -> Result<String, String> {
    let response = ureq::get(url)
        .set("User-Agent", USER_AGENT)
        .call()
        .map_err(|error| error.to_string())?;
    read_response_text(response)
}

pub fn get_bytes(url: &str, referer: Option<&str>) -> Result<Vec<u8>, String> {
    get_bytes_with_headers(url, referer, &std::collections::HashMap::new())
}

pub fn get_bytes_with_headers(
    url: &str,
    referer: Option<&str>,
    headers: &std::collections::HashMap<String, String>,
) -> Result<Vec<u8>, String> {
    let mut request = ureq::get(url).set("User-Agent", USER_AGENT);
    if let Some(referer) = referer {
        request = request.set("Referer", referer);
    }
    for (name, value) in headers {
        request = request.set(name, value);
    }
    let response = request.call().map_err(|error| error.to_string())?;
    let mut data = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut data)
        .map_err(|error| error.to_string())?;
    Ok(data)
}

/// ".jpg" style extension (with the dot) taken from an image URL.
pub fn image_extension(url: &str) -> String {
    url.split(['?', '#'])
        .next()
        .and_then(|path| path.rsplit('/').next())
        .and_then(|name| name.rsplit_once('.'))
        .map(|(_, extension)| extension)
        .filter(|extension| {
            (1..=5).contains(&extension.len())
                && extension
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric())
        })
        .map(|extension| format!(".{}", extension.to_ascii_lowercase()))
        .unwrap_or_else(|| ".jpg".to_string())
}

pub fn read_response_text(response: ureq::Response) -> Result<String, String> {
    let mut text = String::new();
    response
        .into_reader()
        .read_to_string(&mut text)
        .map_err(|error| error.to_string())?;
    Ok(text)
}

pub fn id_from_href<'a>(href: &'a str, route: &str) -> &'a str {
    href.split_once(route)
        .map(|(_, remainder)| remainder)
        .unwrap_or_default()
        .split(['/', '?'])
        .next()
        .unwrap_or_default()
}

pub fn normalize_whitespace(value: String) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn absolute_url(base_url: &str, value: &str) -> String {
    let url = value.split_whitespace().next().unwrap_or_default();
    if url.starts_with("http://") || url.starts_with("https://") {
        url.to_string()
    } else if url.starts_with("//") {
        format!("https:{url}")
    } else if url.starts_with('/') {
        format!("{base_url}{url}")
    } else if url.is_empty() {
        String::new()
    } else {
        format!("{base_url}/{url}")
    }
}

pub fn validate_id(value: &str) -> Result<(), String> {
    if value.is_empty()
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '-' || character == '_'
        })
    {
        return Err("Invalid series or chapter ID".to_string());
    }
    Ok(())
}

pub fn safe_filename(value: &str) -> String {
    let filename: String = value
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
                )
            {
                '_'
            } else {
                character
            }
        })
        .collect();
    let filename = filename
        .trim()
        .trim_matches(|character| character == '.' || character == ' ');
    if filename.is_empty() {
        "chapter".to_string()
    } else {
        filename.to_string()
    }
}

#[cfg(test)]
#[path = "../tests/unit/util/mod.rs"]
mod tests;
