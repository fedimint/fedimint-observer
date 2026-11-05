use fedimint_core::config::FederationId;
use nostr_sdk::{EventBuilder, Keys, Tag};

use super::{ParsedRecommendationEvent, StoredEvent, RECOMMENDATION_EVENT_KIND};

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
