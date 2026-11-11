//! Bounded keyset pagination; response shapes stay compatible with existing
//! clients.

use base64::Engine as _;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// Largest page any list endpoint serves
pub const MAX_PAGE_LIMIT: u16 = 500;
/// Page size when the client names none
pub const DEFAULT_PAGE_LIMIT: u16 = 100;

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
        page.last().map(|last| encode_cursor(&key(last)))
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
    // Cursors are short; anything long is not one of ours
    let invalid = || crate::error::ClientError::bad_request("Invalid page cursor");
    if cursor.len() > 1024 {
        return Err(invalid().into());
    }
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|_| invalid())?;
    serde_json::from_slice(&bytes).map_err(|_| invalid().into())
}

#[cfg(test)]
mod tests;
