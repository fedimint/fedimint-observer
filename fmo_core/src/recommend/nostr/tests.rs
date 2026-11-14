use fedimint_core::config::FederationId;
use nostr_sdk::{EventBuilder, Keys, Tag};

use super::{
    parse_submitted, ParsedFederationEvent, ParsedRecommendationEvent, StoredEvent,
    RECOMMENDATION_EVENT_KIND,
};
use crate::error::ClientError;

#[test]
fn only_correctly_signed_ratings_are_accepted() {
    let federation_id = FederationId::dummy();
    let event = EventBuilder::new(
        RECOMMENDATION_EVENT_KIND,
        "[5/5] great",
        [Tag::identifier(federation_id.to_string())],
    )
    .to_event(&Keys::generate())
    .unwrap();

    let parsed = ParsedRecommendationEvent::parse(&event).unwrap();
    assert_eq!(parsed.federation_id, federation_id);
    assert_eq!(parsed.star_vote, Some(5));

    // What a PUT body can carry: the signed event with its content edited.
    // Deserializing it succeeds, so the signature check must catch it.
    let mut json = serde_json::to_value(&event).unwrap();
    json["content"] = "[1/5] terrible".into();
    let forged: nostr_sdk::Event = serde_json::from_value(json).unwrap();
    assert!(ParsedRecommendationEvent::parse(&forged).is_err());
}

#[test]
fn bad_submitted_events_are_client_errors() {
    let federation_id = FederationId::dummy();
    let rating = EventBuilder::new(
        RECOMMENDATION_EVENT_KIND,
        "[4/5] fine",
        [Tag::identifier(federation_id.to_string())],
    )
    .to_event(&Keys::generate())
    .unwrap();
    let status = |error: anyhow::Error| error.downcast_ref::<ClientError>().unwrap().status();

    // A rating sent as a federation announcement: wrong kind
    let error = parse_submitted::<ParsedFederationEvent>(&rating).unwrap_err();
    assert_eq!(status(error), axum::http::StatusCode::BAD_REQUEST);

    let mut json = serde_json::to_value(&rating).unwrap();
    json["content"] = "[1/5] terrible".into();
    let forged: nostr_sdk::Event = serde_json::from_value(json).unwrap();
    let error = parse_submitted::<ParsedRecommendationEvent>(&forged).unwrap_err();
    assert_eq!(status(error), axum::http::StatusCode::BAD_REQUEST);

    assert!(parse_submitted::<ParsedRecommendationEvent>(&rating).is_ok());
}

#[tokio::test]
#[ignore = "requires an isolated FMO_NOSTR_TEST_DATABASE"]
async fn edited_ratings_count_once() -> anyhow::Result<()> {
    let database = std::env::var("FMO_NOSTR_TEST_DATABASE")?;
    let (client, driver) = tokio_postgres::connect(&database, tokio_postgres::NoTls).await?;
    let driver = tokio::spawn(driver);
    // A schema of the test's own, so nothing else is touched
    let schema = format!("fmo_nostr_test_{}", std::process::id());
    client
        .batch_execute(&format!(
            "CREATE SCHEMA {schema}; SET search_path TO {schema};
             CREATE TABLE nostr_votes (event_id BYTEA PRIMARY KEY, federation_id BYTEA NOT NULL,
                 star_vote INTEGER, event JSONB NOT NULL);
             INSERT INTO nostr_votes VALUES
                 -- Alice rated 1, then edited it to 5
                 ('\\x01', '\\xaa', 1, '{{\"pubkey\": \"alice\", \"created_at\": 100}}'),
                 ('\\x02', '\\xaa', 5, '{{\"pubkey\": \"alice\", \"created_at\": 200}}'),
                 -- Bob's two versions share a timestamp: the lower id wins
                 ('\\x04', '\\xaa', 2, '{{\"pubkey\": \"bob\", \"created_at\": 300}}'),
                 ('\\x03', '\\xaa', 4, '{{\"pubkey\": \"bob\", \"created_at\": 300}}'),
                 -- Carol replaced her rating with a comment without stars
                 ('\\x05', '\\xaa', 3, '{{\"pubkey\": \"carol\", \"created_at\": 100}}'),
                 ('\\x06', '\\xaa', NULL, '{{\"pubkey\": \"carol\", \"created_at\": 101}}'),
                 -- Alice's vote on another federation is a separate one
                 ('\\x07', '\\xbb', 2, '{{\"pubkey\": \"alice\", \"created_at\": 50}}');"
        ))
        .await?;
    let ratings = client
        .query(super::FEDERATION_RATINGS_SQL, &[])
        .await?
        .iter()
        .map(|row| {
            (
                row.get::<_, Vec<u8>>(0),
                (row.get::<_, i64>(1), row.get::<_, Option<f64>>(2)),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    client
        .batch_execute(&format!("DROP SCHEMA {schema} CASCADE"))
        .await?;
    drop(client);
    driver.await??;

    assert_eq!(ratings[&vec![0xaa]], (2, Some(4.5)));
    assert_eq!(ratings[&vec![0xbb]], (1, Some(2.0)));
    Ok(())
}
