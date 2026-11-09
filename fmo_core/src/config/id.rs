use axum::extract::Path;
use axum::Json;
use fedimint_core::config::FederationId;
use fedimint_core::invite_code::InviteCode;

pub async fn fetch_federation_id(
    Path(invite): Path<InviteCode>,
) -> crate::error::Result<Json<FederationId>> {
    Ok(invite.federation_id().into())
}
