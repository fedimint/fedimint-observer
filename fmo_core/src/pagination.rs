//! Bounded keyset pagination; response shapes stay compatible with existing
//! clients.

use base64::Engine as _;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// Largest page any list endpoint serves
pub const MAX_PAGE_LIMIT: u16 = 500;
/// Page size when the client names none
pub const DEFAULT_PAGE_LIMIT: u16 = 100;
/// Text in a sort key is cut to this many characters by [`key_text`]
pub const KEY_TEXT_CHARS: usize = 64;
/// Longest cursor accepted. Every cursor the server generates is far shorter:
/// sort keys hold numbers, fixed-size ids and at most two texts cut by
/// [`key_text`], so even text that JSON escapes 6 bytes per character fits.
pub const MAX_CURSOR_BYTES: usize = 2048;

/// `text` as it goes into a sort key: at most [`KEY_TEXT_CHARS`] characters.
///
/// The cursor is the last row's key, so text taken whole (a federation or
/// guardian name of any length) could produce a cursor longer than the server
/// accepts, ending paging at that row. Rows whose cut texts tie are still
/// ordered by the id every key ends with.
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

/// One page of a list ordered by a computed key, with the cursor of the next
pub struct KeysetPage<T> {
    pub items: Vec<T>,
    /// Opaque cursor for the following page; `None` on the last page
    pub next_after: Option<String>,
    /// Rows in the whole (filtered) list, across all pages
    pub total: usize,
}

/// Pages a list whose order is computed rather than stored (health, uptime,
/// freshness), so it cannot be paged in SQL.
///
/// Rows are ordered by `key` ascending; a key must be unique per row (end it
/// with the row's id) so the order is total. The cursor is the last row's key,
/// encoded opaquely: the next page starts strictly after it, so rows that
/// appear, disappear or move between requests never shift a page sideways
/// the way an offset would.
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
    // Anything longer is not one of ours
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
