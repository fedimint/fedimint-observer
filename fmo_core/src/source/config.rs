//! Push sources declared at deploy time, in the `FO_SOURCES_FILE` document:
//!
//! ```json
//! {
//!   "sources": [
//!     {
//!       "id": "uptime_probe",
//!       "kinds": [
//!         { "name": "guardian_latency", "public": true, "max_bytes": 8192, "retention_hours": 168 }
//!       ]
//!     }
//!   ]
//! }
//! ```
//!
//! The file holds no secrets: each source's token comes from
//! `FO_SOURCE_TOKEN_<ID>`, its id in upper case. A missing or short token
//! stops startup rather than leaving the source open or silently off.

use std::path::Path;
use std::time::Duration;

use anyhow::Context;
use serde::Deserialize;

use super::{ObservationSource, PushToken, SourceKind};

/// The largest sources file read, far above any sensible one
const MAX_FILE_BYTES: u64 = 1024 * 1024;

/// A source outside the build that pushes its observations over HTTP
#[derive(Debug)]
pub struct PushSource {
    id: String,
    kinds: Vec<SourceKind>,
    token: PushToken,
}

impl PushSource {
    pub fn new(id: impl Into<String>, kinds: Vec<SourceKind>, token: PushToken) -> Self {
        Self {
            id: id.into(),
            kinds,
            token,
        }
    }
}

impl ObservationSource for PushSource {
    fn id(&self) -> &str {
        &self.id
    }

    fn kinds(&self) -> Vec<SourceKind> {
        self.kinds.clone()
    }

    fn push_token(&self) -> Option<&PushToken> {
        Some(&self.token)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourcesFile {
    sources: Vec<SourceConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceConfig {
    id: String,
    kinds: Vec<KindConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct KindConfig {
    name: String,
    #[serde(default)]
    public: bool,
    max_bytes: Option<usize>,
    retention_hours: Option<u64>,
}

/// The environment variable holding a push source's token
pub(crate) fn token_variable(id: &str) -> String {
    format!("FO_SOURCE_TOKEN_{}", id.to_ascii_uppercase())
}

/// Reads the push sources of a sources file, with their tokens from `env`.
/// The registry validates ids and kinds when they are added.
pub(crate) fn load_push_sources(
    path: &Path,
    env: impl Fn(&str) -> Option<String>,
) -> anyhow::Result<Vec<PushSource>> {
    let size = std::fs::metadata(path)
        .with_context(|| format!("Reading sources file {}", path.display()))?
        .len();
    anyhow::ensure!(
        size <= MAX_FILE_BYTES,
        "Sources file {} is over {MAX_FILE_BYTES} bytes",
        path.display()
    );
    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("Reading sources file {}", path.display()))?;
    parse_push_sources(&contents, env)
        .with_context(|| format!("Invalid sources file {}", path.display()))
}

pub(super) fn parse_push_sources(
    contents: &str,
    env: impl Fn(&str) -> Option<String>,
) -> anyhow::Result<Vec<PushSource>> {
    let file: SourcesFile = serde_json::from_str(contents)?;
    file.sources
        .into_iter()
        .map(|source| {
            let variable = token_variable(&source.id);
            let secret = env(&variable)
                .with_context(|| format!("Set {variable} to the token of source {}", source.id))?;
            let token = PushToken::new(&secret)
                .with_context(|| format!("{variable} is not a valid token"))?;
            let kinds = source
                .kinds
                .into_iter()
                .map(|kind| {
                    let mut declared = SourceKind::new(kind.name);
                    if kind.public {
                        declared = declared.public();
                    }
                    if let Some(max_bytes) = kind.max_bytes {
                        declared = declared.max_bytes(max_bytes);
                    }
                    if let Some(hours) = kind.retention_hours {
                        declared =
                            declared.retention(Duration::from_secs(hours.saturating_mul(60 * 60)));
                    }
                    declared
                })
                .collect();
            Ok(PushSource::new(source.id, kinds, token))
        })
        .collect()
}
