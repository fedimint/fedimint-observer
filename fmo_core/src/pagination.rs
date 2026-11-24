use base64::Engine as _;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub const MAX_PAGE_LIMIT: u16 = 500;
pub const DEFAULT_PAGE_LIMIT: u16 = 100;
pub const KEY_TEXT_CHARS: usize = 64;
pub const MAX_CURSOR_BYTES: usize = 2048;

pub fn key_text(text: &str) -> String {
    text.chars().take(KEY_TEXT_CHARS).collect()
}

#[derive(Debug, Deserialize)]
pub struct Page<C> {
    pub after: Option<C>,
    pub limit: Option<u16>,
}

impl<C> Page<C> {
    pub fn limit(&self) -> crate::error::Result<i64> {
        let limit = self.limit.unwrap_or(DEFAULT_PAGE_LIMIT);
        if !(1..=MAX_PAGE_LIMIT).contains(&limit) {
            return Err(crate::error::AppError::bad_request(
                "limit must be between 1 and 500",
            ));
        }
        Ok(i64::from(limit))
    }
}

pub struct KeysetPage<T> {
    pub items: Vec<T>,
    pub next_after: Option<String>,
    pub total: usize,
}

/// Keyset-pages lists ordered by a unique computed key.
/// Each page starts after its cursor, avoiding offset drift.
pub fn keyset_page<T, K>(
    mut items: Vec<T>,
    key: impl Fn(&T) -> K,
    after: Option<&str>,
    limit: i64,
) -> anyhow::Result<KeysetPage<T>>
where
    K: Ord + Serialize + DeserializeOwned,
{
    let limit = usize::try_from(limit).unwrap_or(usize::MAX);
    let total = items.len();
    items.sort_by_cached_key(|item| key(item));
    let start = match after {
        None => 0,
        Some(cursor) => {
            let after_key: K = decode_cursor(cursor)?;
            items.partition_point(|item| key(item) <= after_key)
        }
    };
    let mut page: Vec<T> = items.into_iter().skip(start).take(limit + 1).collect();
    let next_after = if page.len() > limit {
        page.truncate(limit);
        page.last().map(|last| {
            let cursor = encode_cursor(&key(last));
            debug_assert!(
                cursor.len() <= MAX_CURSOR_BYTES,
                "sort keys must cut their text with key_text"
            );
            cursor
        })
    } else {
        None
    };
    Ok(KeysetPage {
        items: page,
        next_after,
        total,
    })
}

fn encode_cursor<K: Serialize>(key: &K) -> String {
    let json = serde_json::to_vec(key).expect("cursor keys serialize");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json)
}

fn decode_cursor<K: DeserializeOwned>(cursor: &str) -> anyhow::Result<K> {
    let invalid = || crate::error::ClientError::bad_request("Invalid page cursor");
    if cursor.len() > MAX_CURSOR_BYTES {
        return Err(invalid().into());
    }
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|_| invalid())?;
    serde_json::from_slice(&bytes).map_err(|_| invalid().into())
}

#[cfg(test)]
mod tests;
