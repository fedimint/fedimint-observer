INSERT INTO schema_version (version)
VALUES (4);

CREATE TABLE IF NOT EXISTS nostr_votes (
    event_id BYTEA NOT NULL PRIMARY KEY,
    federation_id BYTEA NOT NULL REFERENCES federations(federation_id),
    star_vote INTEGER,
    event JSONB NOT NULL,
    fetch_time TIMESTAMP NOT NULL
);
CREATE INDEX IF NOT EXISTS nostr_votes_federation ON nostr_votes(federation_id);
CREATE INDEX IF NOT EXISTS nostr_votes_fetch_time ON nostr_votes(fetch_time);

CREATE TABLE IF NOT EXISTS nostr_relays (
    relay_url TEXT NOT NULL PRIMARY KEY
);
INSERT INTO nostr_relays (relay_url) VALUES ('wss://relay.damus.io'),
                                            ('wss://nostr.bitcoiner.social/'),
                                            ('wss://relay.nostr.info/'),
                                            ('wss://nostr-01.bolt.observer/'),
                                            ('wss://nostr.mutinywallet.com/'),
                                            ('wss://relay.snort.social/'),
                                            ('wss://relay.primal.net/'),
                                            ('wss://relay.satoshidnc.com/'),
                                            ('wss://nos.lol/'),
                                            ('wss://nostr-pub.wellorder.net/') ON CONFLICT DO NOTHING;
