use std::str::FromStr;

use fedimint_core::config::FederationId;
use fedimint_core::encoding::Encodable;

use super::federation_session_ranges;

#[tokio::test]
#[ignore = "requires an isolated FMO_WORK_TEST_DATABASE"]
async fn postgres_federation_session_ranges_use_dated_bounds() -> anyhow::Result<()> {
    let database = std::env::var("FMO_WORK_TEST_DATABASE")?;
    let pool = crate::runtime::pool(&database, 1, false)?;
    let client = pool.get().await?;
    let first_id =
        FederationId::from_str("4b13a146ee4ba732b2b8914a72a0a2e5873e3e942da2d4eeefd85a5fe41f27ba")?;
    let undated_id =
        FederationId::from_str("713d9b889b5e207a85d8632e2e5f6926cf74c532299bc313a5fc105534007fce")?;
    let invalid_id =
        FederationId::from_str("06c89f746e32945b5775e6a9780f48751fc96bdecbc67e22de4c04d9a1105f40")?;
    client
        .batch_execute(
            "CREATE TEMP TABLE session_times(
                 federation_id BYTEA NOT NULL,
                 session_index INTEGER NOT NULL,
                 estimated_session_timestamp TIMESTAMP
             );
             CREATE INDEX session_times_federation_estimated
                 ON session_times(federation_id, estimated_session_timestamp);",
        )
        .await?;
    client
        .execute(
            "INSERT INTO session_times VALUES
                 ($1, 0, NULL),
                 ($1, 1, TIMESTAMP '2024-05-10 21:08:34'),
                 ($1, 2, TIMESTAMP '2024-09-10 16:07:52'),
                 ($2, 0, NULL),
                 ($3, 0, TIMESTAMP '1969-12-31 23:59:59'),
                 ($3, 1, TIMESTAMP '2024-09-10 16:07:52')",
            &[
                &first_id.consensus_encode_to_vec(),
                &undated_id.consensus_encode_to_vec(),
                &invalid_id.consensus_encode_to_vec(),
            ],
        )
        .await?;

    let federation_ids = [first_id, undated_id, invalid_id].map(|id| id.consensus_encode_to_vec());
    let ranges = federation_session_ranges(&client, &federation_ids).await?;
    assert_eq!(
        ranges.get(&first_id),
        Some(&(Some(1_715_375_314), Some(1_725_984_472)))
    );
    assert_eq!(ranges.get(&undated_id), Some(&(None, None)));
    assert_eq!(ranges.get(&invalid_id), Some(&(None, None)));
    Ok(())
}
