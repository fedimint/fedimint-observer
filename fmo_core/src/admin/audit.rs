//! The record of what admins did (`admin_audit_log`), and the limit on how
//! many federations a delegate adds per day, which is counted from it.
//!
//! An entry is written before its action runs, so an action that was started
//! is on record even if the process stops halfway; its outcome is set after.

use std::future::Future;

use chrono::{DateTime, Utc};
use deadpool_postgres::GenericClient;
use fmo_api_types::{AdminAuditEntry, AdminRole};
use nostr_sdk::PublicKey;
use serde_json::Value;
use tracing::{info, warn};

use super::auth::Admin;
use super::{key_id, role_name};
use crate::error::{AppError, ClientError};
use crate::federation::observer::FederationObserver;

/// Federations a delegate may ask to add in any 24 hours, successful or not:
/// every attempt connects to the guardians an invite code names, and every
/// success starts collecting a federation for good.
pub(super) const DELEGATE_ADDS_PER_DAY: i64 = 20;

/// What an admin did. The names are stored, so they never change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    FederationAdd,
    FederationBackfill,
    ModuleReset,
    KeyGrant,
    KeyRevoke,
}

impl Action {
    fn as_str(self) -> &'static str {
        match self {
            Action::FederationAdd => "federation.add",
            Action::FederationBackfill => "federation.backfill",
            Action::ModuleReset => "module.reset",
            Action::KeyGrant => "key.grant",
            Action::KeyRevoke => "key.revoke",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Outcome {
    Started,
    Succeeded,
    Failed,
}

impl Outcome {
    fn as_str(self) -> &'static str {
        match self {
            Outcome::Started => "started",
            Outcome::Succeeded => "succeeded",
            Outcome::Failed => "failed",
        }
    }
}

/// Writes an entry and returns its id. `detail` must hold no secrets: invite
/// codes can carry an API secret, so only federation ids are recorded.
pub(super) async fn insert(
    client: &impl GenericClient,
    admin: &Admin,
    action: Action,
    subject: &str,
    detail: &Value,
    outcome: Outcome,
) -> anyhow::Result<i64> {
    let actor = admin.principal.key().map(PublicKey::to_bytes);
    let id = client
        .query_one(
            "INSERT INTO admin_audit_log (actor, actor_role, action, subject, detail, outcome)
             VALUES ($1, $2, $3, $4, $5, $6)
             RETURNING id",
            &[
                &actor.as_ref().map(<[u8; 32]>::as_slice),
                &role_name(admin.role),
                &action.as_str(),
                &subject,
                detail,
                &outcome.as_str(),
            ],
        )
        .await?
        .try_get(0)?;
    Ok(id)
}

/// Records that `admin` starts `action`, after checking a delegate's daily
/// limit of federations.
///
/// The check and the entry share a transaction that locks the delegate's
/// grant: its concurrent requests wait for each other, so they can't pass the
/// limit together, and a grant revoked meanwhile is refused here.
async fn begin(
    observer: &FederationObserver,
    admin: &Admin,
    action: Action,
    subject: &str,
    detail: &Value,
) -> anyhow::Result<i64> {
    let mut connection = observer.connection().await?;
    let transaction = connection.transaction().await?;
    if let (AdminRole::Delegate, Some(key)) = (admin.role, admin.principal.key()) {
        let key = key.to_bytes();
        let granted = transaction
            .query_opt(
                "SELECT 1 FROM admin_keys
                 WHERE pubkey = $1 AND (expires_at IS NULL OR expires_at > now())
                 FOR UPDATE",
                &[&key.as_slice()],
            )
            .await?
            .is_some();
        if !granted {
            return Err(ClientError::forbidden("This key's access was revoked").into());
        }
        if action == Action::FederationAdd {
            let recent: i64 = transaction
                .query_one(
                    "SELECT COUNT(*) FROM admin_audit_log
                     WHERE actor = $1 AND action = $2 AND at > now() - interval '24 hours'",
                    &[&key.as_slice(), &action.as_str()],
                )
                .await?
                .try_get(0)?;
            if recent >= DELEGATE_ADDS_PER_DAY {
                return Err(ClientError::too_many_requests(format!(
                    "This key added {DELEGATE_ADDS_PER_DAY} federations in the last 24 hours; try again later"
                ))
                .into());
            }
        }
    }
    let id = insert(
        &transaction,
        admin,
        action,
        subject,
        detail,
        Outcome::Started,
    )
    .await?;
    transaction.commit().await?;
    Ok(id)
}

/// Runs `work` as the audited `action` of `admin`: records it first, then its
/// outcome. A request cancelled at its deadline leaves the entry `started`.
pub(crate) async fn audited<T>(
    observer: &FederationObserver,
    admin: &Admin,
    action: Action,
    subject: &str,
    detail: Value,
    work: impl Future<Output = anyhow::Result<T>>,
) -> anyhow::Result<T> {
    let id = begin(observer, admin, action, subject, &detail).await?;
    info!(
        audit_id = id,
        action = action.as_str(),
        subject,
        role = role_name(admin.role),
        actor = %admin.principal.key().map_or_else(|| "token".to_owned(), PublicKey::to_hex),
        "Admin action"
    );
    let result = work.await;
    let outcome = if result.is_ok() {
        Outcome::Succeeded
    } else {
        Outcome::Failed
    };
    // The action is done either way; a lost outcome only leaves it `started`
    let stored = async {
        observer
            .connection()
            .await?
            .execute(
                "UPDATE admin_audit_log SET outcome = $2 WHERE id = $1 AND outcome = 'started'",
                &[&id, &outcome.as_str()],
            )
            .await?;
        anyhow::Ok(())
    };
    if let Err(error) = stored.await {
        warn!(
            audit_id = id,
            ?error,
            "Could not store an admin action's outcome"
        );
    }
    result
}

/// A page of the log, newest first: entries older than `before`, and the
/// cursor of the next page if there may be one
pub(super) async fn list(
    client: &impl GenericClient,
    before: Option<i64>,
    limit: i64,
) -> Result<(Vec<AdminAuditEntry>, Option<i64>), AppError> {
    let rows = client
        .query(
            "SELECT id, at, actor, actor_role, action, subject, detail, outcome
             FROM admin_audit_log
             WHERE $1::bigint IS NULL OR id < $1
             ORDER BY id DESC
             LIMIT $2",
            &[&before, &limit],
        )
        .await?;
    let entries = rows
        .iter()
        .map(|row| {
            let actor = row
                .try_get::<_, Option<Vec<u8>>>("actor")?
                .map(|bytes| PublicKey::from_slice(&bytes))
                .transpose()?;
            let role: String = row.try_get("actor_role")?;
            let action: String = row.try_get("action")?;
            let subject: String = row.try_get("subject")?;
            let subject_key = action
                .starts_with("key.")
                .then(|| PublicKey::from_hex(&subject).ok())
                .flatten();
            anyhow::Ok(AdminAuditEntry {
                id: row.try_get("id")?,
                at: row.try_get::<_, DateTime<Utc>>("at")?,
                actor: actor.as_ref().map(key_id),
                actor_role: if role == "owner" {
                    AdminRole::Owner
                } else {
                    AdminRole::Delegate
                },
                action,
                subject,
                subject_key: subject_key.as_ref().map(key_id),
                detail: row.try_get("detail")?,
                outcome: row.try_get("outcome")?,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let next = (entries.len() as i64 == limit)
        .then(|| entries.last().map(|entry| entry.id))
        .flatten();
    Ok((entries, next))
}
