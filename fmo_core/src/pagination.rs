//! Bounded keyset pagination; response shapes stay compatible with existing
//! clients.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Page<C> {
    pub after: Option<C>,
    pub limit: Option<u16>,
}

impl<C> Page<C> {
    pub fn limit(&self) -> crate::error::Result<i64> {
        let limit = self.limit.unwrap_or(100);
        if !(1..=500).contains(&limit) {
            return Err(crate::error::AppError::bad_request(
                "limit must be between 1 and 500",
            ));
        }
        Ok(i64::from(limit))
    }
}

#[cfg(test)]
mod tests;
