CREATE TABLE IF NOT EXISTS federations
(
    federation_id BYTEA PRIMARY KEY NOT NULL,
    config        BYTEA             NOT NULL
);

CREATE TABLE IF NOT EXISTS sessions
(
    federation_id BYTEA   NOT NULL REFERENCES federations (federation_id),
    session_index INTEGER NOT NULL,
    -- TODO: add transaction and item count
    session       BYTEA   NOT NULL,
    PRIMARY KEY (federation_id, session_index)
);
CREATE INDEX IF NOT EXISTS federation_sessions ON sessions (federation_id);

CREATE TABLE IF NOT EXISTS transactions
(
    txid          BYTEA   NOT NULL,
    federation_id BYTEA   NOT NULL REFERENCES federations (federation_id),
    session_index INTEGER NOT NULL,
    item_index    INTEGER NOT NULL,
    data          BYTEA   NOT NULL,
    FOREIGN KEY (federation_id, session_index) REFERENCES sessions (federation_id, session_index),
    PRIMARY KEY (federation_id, txid)
);
CREATE INDEX IF NOT EXISTS federation_transactions ON transactions (federation_id);
CREATE INDEX IF NOT EXISTS federation_session_transactions ON transactions (federation_id, session_index);

CREATE TABLE IF NOT EXISTS ln_contracts
(
    federation_id BYTEA NOT NULL REFERENCES federations (federation_id),
    contract_id   BYTEA NOT NULL,
    type          TEXT  NOT NULL CHECK (type IN ('incoming', 'outgoing')),
    payment_hash  BYTEA NOT NULL,
    PRIMARY KEY (federation_id, contract_id)
);
CREATE INDEX IF NOT EXISTS ln_contract_federation_contract ON ln_contracts (federation_id, contract_id);
CREATE INDEX IF NOT EXISTS ln_contract_federation ON ln_contracts (federation_id);
CREATE INDEX IF NOT EXISTS ln_contract_hashes ON ln_contracts (payment_hash);

CREATE TABLE IF NOT EXISTS transaction_inputs
(
    federation_id  BYTEA   NOT NULL REFERENCES federations (federation_id),
    txid           BYTEA   NOT NULL,
    in_index       INTEGER NOT NULL,
    kind           TEXT    NOT NULL,
    ln_contract_id BYTEA,
    amount_msat    BIGINT,
    PRIMARY KEY (federation_id, txid, in_index),
    FOREIGN KEY (federation_id, txid) REFERENCES transactions (federation_id, txid) -- This might be a bit too heavy of a foreign key? Maybe use rowid instead?
    -- Can't apply the following FK constraint because contract can be null:
    -- FOREIGN KEY (federation_id, ln_contract_id) REFERENCES ln_contracts(federation_id, contract_id)
);
CREATE INDEX IF NOT EXISTS federation_inputs ON transaction_inputs (federation_id);
CREATE INDEX IF NOT EXISTS federation_transaction_inputs ON transaction_inputs (federation_id, txid);
CREATE INDEX IF NOT EXISTS federation_input_kinds ON transaction_inputs (federation_id, kind);

CREATE TABLE IF NOT EXISTS transaction_outputs
(
    federation_id                BYTEA   NOT NULL REFERENCES federations (federation_id),
    txid                         BYTEA   NOT NULL,
    out_index                    INTEGER NOT NULL,
    kind                         TEXT    NOT NULL,
    -- We keep the ln contract relation denormalized for now. If additional modules need extra data attached to
    -- inputs/outputs we'll have to refactor that or introduce some constraints to keep the complexity manageable.
    ln_contract_interaction_kind TEXT CHECK (ln_contract_interaction_kind IN ('fund', 'cancel', 'offer', NULL)),
    ln_contract_id               BYTEA,
    amount_msat                  BIGINT,
    PRIMARY KEY (federation_id, txid, out_index),
    FOREIGN KEY (federation_id, txid) REFERENCES transactions (federation_id, txid) -- This might be a bit too heavy of a foreign key? Maybe use rowid instead?
    -- Can't apply the following FK constraint because contract doesn't exist yet when offers are created:
    -- FOREIGN KEY (federation_id, ln_contract_id) REFERENCES ln_contracts(federation_id, contract_id)
);
CREATE INDEX IF NOT EXISTS federation_outputs ON transaction_outputs (federation_id);
CREATE INDEX IF NOT EXISTS federation_transaction_outputs ON transaction_outputs (federation_id, txid);
CREATE INDEX IF NOT EXISTS federation_output_kinds ON transaction_outputs (federation_id, kind);


CREATE TABLE IF NOT EXISTS block_times
(
    block_height INTEGER PRIMARY KEY,
    timestamp    TIMESTAMP NOT NULL
);
CREATE INDEX IF NOT EXISTS block_times_time ON block_times (timestamp);

CREATE TABLE IF NOT EXISTS block_height_votes
(
    federation_id BYTEA   NOT NULL REFERENCES federations (federation_id),
    session_index INTEGER NOT NULL,
    item_index    INTEGER NOT NULL,
    proposer      INTEGER NOT NULL,
    height_vote   INTEGER NOT NULL REFERENCES block_times (block_height),
    PRIMARY KEY (federation_id, session_index, item_index),
    FOREIGN KEY (federation_id, session_index) REFERENCES sessions (federation_id, session_index)
);
CREATE INDEX IF NOT EXISTS block_height_vote_federation_sessions ON block_height_votes (federation_id, session_index);
CREATE INDEX IF NOT EXISTS block_height_vote_heights ON block_height_votes (height_vote);

CREATE
OR REPLACE
VIEW session_times AS
WITH session_votes AS (
    SELECT
        s.session_index,
        s.federation_id
    FROM
        sessions s
), sorted_votes AS (
    SELECT
        sv.session_index,
        sv.federation_id,
        height_vote,
        ROW_NUMBER() OVER (PARTITION BY sv.federation_id, sv.session_index ORDER BY height_vote) AS rn,
        COUNT(bhv.height_vote) OVER (PARTITION BY sv.federation_id, sv.session_index) AS total_votes
    FROM
        session_votes sv
            LEFT JOIN
        block_height_votes bhv ON sv.session_index = bhv.session_index
            AND sv.federation_id = bhv.federation_id
), session_max_height AS (
    SELECT
        session_index,
        federation_id,
        MAX(height_vote) AS max_height_vote -- Include max to handle NULLs in averaging
    FROM
        sorted_votes
    WHERE
        total_votes > 0
    GROUP BY
        federation_id, session_index
), session_time AS (
    SELECT
        sv.session_index,
        sv.federation_id,
        (
            SELECT
                bt.timestamp
            FROM
                block_times bt
            WHERE
                mh.max_height_vote IS NOT NULL
              AND bt.block_height = mh.max_height_vote
        ) AS timestamp
    FROM
        session_votes sv
            LEFT JOIN
        session_max_height mh ON sv.session_index = mh.session_index AND sv.federation_id = mh.federation_id
), grouped_sessions AS (
    SELECT
        *,
        SUM(CASE WHEN timestamp IS NOT NULL THEN 1 ELSE 0 END) OVER (PARTITION BY federation_id ORDER BY session_index) AS time_group
    FROM
        session_time
), propagated_times AS (
    SELECT
        session_index,
        federation_id,
        FIRST_VALUE(timestamp) OVER (PARTITION BY federation_id, time_group ORDER BY session_index) AS estimated_session_timestamp
    FROM
        grouped_sessions
)
SELECT federation_id,
       session_index,
       estimated_session_timestamp
FROM propagated_times
ORDER BY federation_id, session_index;
