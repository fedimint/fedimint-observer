//! Storage of delegated access (`admin_keys`) and of the signed requests
//! already used (`admin_auth_events`).

use anyhow::Context;
use chrono::{DateTime, Utc};
use deadpool_postgres::GenericClient;
use nostr_sdk::PublicKey;
use serde_json::json;
use tokio_postgres::Row;

use super::audit::{self, Action, Outcome};
use super::auth::{Admin, MAX_CLOCK_SKEW};
use crate::error::ClientError;

/// Keys that can hold a grant at once. Owners grant by hand; the cap keeps a
/// mistake or a stolen owner key from filling the table, and lets the list be
/// served whole.
pub(super) const MAX_DELEGATES: i64 = 64;

/// The longest label, as the schema checks
const MAX_LABEL_CHARS: usize = 64;

/// A key an owner delegated admin access to
#[derive(Debug, Clone)]
pub(crate) struct Delegate {
    pub pubkey: PublicKey,
    pub label: String,
    /// `None`: granted with the bearer token
    pub granted_by: Option<PublicKey>,
    pub granted_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}

impl Delegate {
    const COLUMNS: &'static str = "pubkey, label, granted_by, granted_at, expires_at";

    fn from_row(row: &Row) -> anyhow::Result<Self> {
        let key = |bytes: Vec<u8>| {
            PublicKey::from_slice(&bytes).context("admin_keys holds an invalid public key")
        };
        Ok(Self {
            pubkey: key(row.try_get("pubkey")?)?,
            label: row.try_get("label")?,
            granted_by: row
                .try_get::<_, Option<Vec<u8>>>("granted_by")?
                .map(key)
                .transpose()?,
            granted_at: row.try_get("granted_at")?,
            expires_at: row.try_get("expires_at")?,
        })
    }

    pub(crate) fn expired(&self, now: DateTime<Utc>) -> bool {
        self.expires_at.is_some_and(|expires_at| expires_at <= now)
    }
}

/// The key's grant, if it has one that hasn't expired
pub(super) async fn active_delegate(
    client: &impl GenericClient,
    pubkey: &PublicKey,
    now: DateTime<Utc>,
) -> anyhow::Result<Option<Delegate>> {
    client
        .query_opt(
            &format!(
                "SELECT {} FROM admin_keys
                 WHERE pubkey = $1 AND (expires_at IS NULL OR expires_at > $2)",
                Delegate::COLUMNS
            ),
            &[&pubkey.to_bytes().as_slice(), &now],
        )
        .await?
        .as_ref()
        .map(Delegate::from_row)
        .transpose()
}

/// Remembers a signed request's event id. `false` if it was used before.
///
/// Ids are kept for twice the accepted clock skew: an event is accepted from
/// `created_at - skew` to `created_at + skew`, so by then it is too old to be
/// accepted again. Ids past that are deleted here, which keeps the table to
/// the admins' requests of the last two minutes.
pub(super) async fn record_signed_request(
    client: &impl GenericClient,
    event_id: &[u8; 32],
    now: DateTime<Utc>,
) -> anyhow::Result<bool> {
    client
        .execute(
            "DELETE FROM admin_auth_events WHERE expires_at < $1",
            &[&now],
        )
        .await?;
    let inserted = client
        .execute(
            "INSERT INTO admin_auth_events (event_id, expires_at) VALUES ($1, $2)
             ON CONFLICT DO NOTHING",
            &[&event_id.as_slice(), &(now + 2 * MAX_CLOCK_SKEW)],
        )
        .await?;
    Ok(inserted == 1)
}

/// Every stored grant, expired ones included so owners can see and remove
/// them. Bounded by [`MAX_DELEGATES`].
pub(super) async fn list_delegates(client: &impl GenericClient) -> anyhow::Result<Vec<Delegate>> {
    client
        .query(
            &format!(
                "SELECT {} FROM admin_keys ORDER BY granted_at DESC, pubkey LIMIT $1",
                Delegate::COLUMNS
            ),
            &[&MAX_DELEGATES],
        )
        .await?
        .iter()
        .map(Delegate::from_row)
        .collect()
}

/// A label as stored: trimmed, 1 to 64 characters, none of them control
/// characters. It is shown to owners as text.
pub(super) fn valid_label(label: &str) -> Result<&str, ClientError> {
    let label = label.trim();
    let length = label.chars().count();
    if !(1..=MAX_LABEL_CHARS).contains(&length) || label.chars().any(char::is_control) {
        return Err(ClientError::bad_request(format!(
            "The label needs 1 to {MAX_LABEL_CHARS} characters"
        )));
    }
    Ok(label)
}

/// Grants `pubkey` delegate access, or replaces its grant's label and expiry.
/// The grant and its audit entry are written together.
pub(super) async fn grant(
    connection: &mut deadpool_postgres::Object,
    admin: &Admin,
    pubkey: &PublicKey,
    label: &str,
    expires_at: Option<DateTime<Utc>>,
) -> anyhow::Result<Delegate> {
    let label = valid_label(label)?;
    let transaction = connection.transaction().await?;
    // Grants wait for each other, so the cap can't be passed by two at once.
    // Lookups of requests being authenticated aren't blocked.
    transaction
        .batch_execute("LOCK TABLE admin_keys IN SHARE ROW EXCLUSIVE MODE")
        .await?;
    let pubkey_bytes = pubkey.to_bytes();
    let (count, exists): (i64, bool) = {
        let row = transaction
            .query_one(
                "SELECT COUNT(*), COALESCE(BOOL_OR(pubkey = $1), false) FROM admin_keys",
                &[&pubkey_bytes.as_slice()],
            )
            .await?;
        (row.try_get(0)?, row.try_get(1)?)
    };
    if !exists && count >= MAX_DELEGATES {
        return Err(ClientError::bad_request(format!(
            "At most {MAX_DELEGATES} keys can hold access; revoke one first"
        ))
        .into());
    }
    let granted_by = admin.principal.key().map(PublicKey::to_bytes);
    let row = transaction
        .query_one(
            &format!(
                "INSERT INTO admin_keys (pubkey, label, granted_by, expires_at)
                 VALUES ($1, $2, $3, $4)
                 ON CONFLICT (pubkey) DO UPDATE SET
                     label = EXCLUDED.label,
                     granted_by = EXCLUDED.granted_by,
                     granted_at = now(),
                     expires_at = EXCLUDED.expires_at
                 RETURNING {}",
                Delegate::COLUMNS
            ),
            &[
                &pubkey_bytes.as_slice(),
                &label,
                &granted_by.as_ref().map(<[u8; 32]>::as_slice),
                &expires_at,
            ],
        )
        .await?;
    audit::insert(
        &transaction,
        admin,
        Action::KeyGrant,
        &pubkey.to_hex(),
        &json!({ "label": label, "expires_at": expires_at }),
        Outcome::Succeeded,
    )
    .await?;
    transaction.commit().await?;
    Delegate::from_row(&row)
}

/// Removes the key's grant; its next request is refused. `false` if it held
/// none.
pub(super) async fn revoke(
    connection: &mut deadpool_postgres::Object,
    admin: &Admin,
    pubkey: &PublicKey,
) -> anyhow::Result<bool> {
    let transaction = connection.transaction().await?;
    let label = transaction
        .query_opt(
            "DELETE FROM admin_keys WHERE pubkey = $1 RETURNING label",
            &[&pubkey.to_bytes().as_slice()],
        )
        .await?
        .map(|row| row.try_get::<_, String>(0))
        .transpose()?;
    let Some(label) = label else {
        return Ok(false);
    };
    audit::insert(
        &transaction,
        admin,
        Action::KeyRevoke,
        &pubkey.to_hex(),
        &json!({ "label": label }),
        Outcome::Succeeded,
    )
    .await?;
    transaction.commit().await?;
    Ok(true)
}
