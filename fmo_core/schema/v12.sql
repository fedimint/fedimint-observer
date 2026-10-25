-- Read models kept up to date as sessions and health samples are written, so
-- page loads and background jobs stop aggregating over the full history

INSERT INTO schema_version (version)
VALUES (12);

-- Block writers until the seeds below are done and the maintenance below
-- takes over, so no row is counted twice or missed
LOCK TABLE sessions, block_height_votes, transactions, transaction_inputs,
    transaction_outputs, guardian_health IN SHARE MODE;

-- Session times
--
-- The materialized view recomputed every session of every federation each
-- minute. A session's time only depends on its own block height votes and the
-- sessions before it, and sessions are committed in order, so it is final once
-- written. The view stays, no longer refreshed, until it is dropped separately.
ALTER MATERIALIZED VIEW session_times RENAME TO session_times_v5;
ALTER INDEX IF EXISTS session_times_federation_id_idx
    RENAME TO session_times_v5_federation_id_idx;
ALTER INDEX IF EXISTS session_times_federation_id_session_index_idx
    RENAME TO session_times_v5_federation_id_session_index_idx;
ALTER INDEX IF EXISTS session_times_federation_id_estimated_session_timestamp_idx
    RENAME TO session_times_v5_federation_id_estimated_session_timestamp_idx;

CREATE TABLE session_times (
    federation_id BYTEA NOT NULL,
    session_index INTEGER NOT NULL,
    -- The height most guardians voted for in the session, NULL without votes
    block_height INTEGER,
    -- The latest block time voted for in this session or an earlier one
    estimated_session_timestamp TIMESTAMP,
    PRIMARY KEY (federation_id, session_index)
);

-- Live sync and backfills of the same federation take this lock before
-- reading what earlier sessions or other writers left behind
CREATE FUNCTION lock_federation_read_models(fed BYTEA) RETURNS VOID AS $$
    SELECT pg_advisory_xact_lock(12, hashtext(encode(fed, 'hex')));
$$ LANGUAGE sql;

-- Writes the times of a federation's sessions `from_index` to `to_index`.
-- Each depends on the sessions before it, so after a change all later ones
-- must be rewritten too.
--
-- Same as the v5 view, except that a tie between heights now goes to the
-- higher one instead of an arbitrary one
CREATE FUNCTION update_session_times(fed BYTEA, from_index INTEGER, to_index INTEGER)
RETURNS VOID AS $$
BEGIN
    PERFORM lock_federation_read_models(fed);

    INSERT INTO session_times (federation_id, session_index, block_height, estimated_session_timestamp)
    WITH proposer_votes AS (
        SELECT session_index, proposer, MAX(height_vote) AS height
        FROM block_height_votes
        WHERE federation_id = fed AND session_index BETWEEN from_index AND to_index
        GROUP BY session_index, proposer
    ),
    session_heights AS (
        SELECT DISTINCT ON (session_index) session_index, height
        FROM proposer_votes
        GROUP BY session_index, height
        ORDER BY session_index, COUNT(*) DESC, height DESC
    ),
    previous AS (
        SELECT st.estimated_session_timestamp
        FROM session_times st
        WHERE st.federation_id = fed AND st.session_index < from_index
        ORDER BY st.session_index DESC
        LIMIT 1
    )
    SELECT fed,
           s.session_index,
           sh.height,
           GREATEST(
               MAX(bt.timestamp) OVER (ORDER BY s.session_index ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW),
               (SELECT estimated_session_timestamp FROM previous)
           )
    FROM sessions s
    LEFT JOIN session_heights sh ON sh.session_index = s.session_index
    LEFT JOIN block_times bt ON bt.block_height = sh.height
    WHERE s.federation_id = fed AND s.session_index BETWEEN from_index AND to_index
    ON CONFLICT (federation_id, session_index) DO UPDATE
        SET block_height = EXCLUDED.block_height,
            estimated_session_timestamp = EXCLUDED.estimated_session_timestamp
        WHERE (session_times.block_height, session_times.estimated_session_timestamp)
            IS DISTINCT FROM (EXCLUDED.block_height, EXCLUDED.estimated_session_timestamp);
END;
$$ LANGUAGE plpgsql;

SELECT update_session_times(federation_id, 0, 2147483647) FROM federations;

-- Added after the seed, which they made three times slower checked row by row
ALTER TABLE session_times
    ADD FOREIGN KEY (federation_id, session_index) REFERENCES sessions (federation_id, session_index);
CREATE INDEX session_times_federation_estimated
    ON session_times (federation_id, estimated_session_timestamp);

-- Daily activity
--
-- Backs the transaction histogram and the federation list's last 7 days,
-- which aggregated all of a federation's transactions on every page load.
-- Sessions without a time yet (before the first block height vote) are left
-- out, as both did.
CREATE TABLE federation_daily_activity (
    federation_id BYTEA NOT NULL REFERENCES federations (federation_id),
    day DATE NOT NULL,
    -- Every transaction, as the federation list counts them
    tx_count BIGINT NOT NULL,
    -- Transactions with at least one input, as the histogram counts them. The
    -- others create outputs only, such as Lightning offers.
    funded_tx_count BIGINT NOT NULL,
    -- Sum of all input amounts
    volume_msat BIGINT NOT NULL,
    PRIMARY KEY (federation_id, day)
);

CREATE FUNCTION session_range_activity(fed BYTEA, from_index INTEGER, to_index INTEGER)
RETURNS TABLE (day DATE, tx_count BIGINT, funded_tx_count BIGINT, volume_msat BIGINT) AS $$
    SELECT DATE(st.estimated_session_timestamp),
           COUNT(DISTINCT t.txid),
           COUNT(DISTINCT t.txid) FILTER (WHERE ti.txid IS NOT NULL),
           COALESCE(SUM(ti.amount_msat), 0)::bigint
    FROM transactions t
    JOIN session_times st
        ON st.federation_id = t.federation_id AND st.session_index = t.session_index
    LEFT JOIN transaction_inputs ti
        ON ti.federation_id = t.federation_id AND ti.txid = t.txid
    WHERE t.federation_id = fed
      AND t.session_index BETWEEN from_index AND to_index
      AND st.estimated_session_timestamp IS NOT NULL
    GROUP BY 1;
$$ LANGUAGE sql STABLE;

-- Recounts a federation from scratch, after a backfill restored rows
CREATE FUNCTION rebuild_daily_activity(fed BYTEA) RETURNS VOID AS $$
BEGIN
    PERFORM lock_federation_read_models(fed);
    DELETE FROM federation_daily_activity WHERE federation_id = fed;
    INSERT INTO federation_daily_activity (federation_id, day, tx_count, funded_tx_count, volume_msat)
    SELECT fed, a.day, a.tx_count, a.funded_tx_count, a.volume_msat
    FROM session_range_activity(fed, 0, 2147483647) a;
END;
$$ LANGUAGE plpgsql;

SELECT rebuild_daily_activity(federation_id) FROM federations;

-- Called by the sync for every session it writes, in the session's
-- transaction. Counts the session's transactions on top of what is there, so
-- it must run exactly once per session: backfills of stored sessions call
-- rebuild_read_models instead.
CREATE FUNCTION record_new_session(fed BYTEA, idx INTEGER) RETURNS VOID AS $$
BEGIN
    PERFORM update_session_times(fed, idx, idx);
    INSERT INTO federation_daily_activity (federation_id, day, tx_count, funded_tx_count, volume_msat)
    SELECT fed, a.day, a.tx_count, a.funded_tx_count, a.volume_msat
    FROM session_range_activity(fed, idx, idx) a
    ON CONFLICT (federation_id, day) DO UPDATE
        SET tx_count = federation_daily_activity.tx_count + EXCLUDED.tx_count,
            funded_tx_count = federation_daily_activity.funded_tx_count + EXCLUDED.funded_tx_count,
            volume_msat = federation_daily_activity.volume_msat + EXCLUDED.volume_msat;
END;
$$ LANGUAGE plpgsql;

-- Rewrites a federation's read models from `from_index` on, for when stored
-- sessions were reprocessed
CREATE FUNCTION rebuild_read_models(fed BYTEA, from_index INTEGER) RETURNS VOID AS $$
BEGIN
    PERFORM update_session_times(fed, from_index, 2147483647);
    PERFORM rebuild_daily_activity(fed);
END;
$$ LANGUAGE plpgsql;

-- Federation assets
--
-- The federation list summed every wallet input and output of each
-- federation on every load. Kept by the v9 counters instead.
ALTER TABLE federation_totals
    ADD COLUMN wallet_in_msat BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN wallet_out_msat BIGINT NOT NULL DEFAULT 0;

CREATE OR REPLACE FUNCTION federation_totals_add_volume() RETURNS TRIGGER AS $$
DECLARE
    amount BIGINT := COALESCE(NEW.amount_msat, 0);
    wallet_amount BIGINT := CASE WHEN NEW.kind = 'wallet' THEN amount ELSE 0 END;
BEGIN
    INSERT INTO federation_totals (federation_id, tx_volume_msat, wallet_in_msat)
    VALUES (NEW.federation_id, amount, wallet_amount)
    ON CONFLICT (federation_id) DO UPDATE
        SET tx_volume_msat = federation_totals.tx_volume_msat + amount,
            wallet_in_msat = federation_totals.wallet_in_msat + wallet_amount;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE FUNCTION federation_totals_add_wallet_out() RETURNS TRIGGER AS $$
BEGIN
    INSERT INTO federation_totals (federation_id, wallet_out_msat)
    VALUES (NEW.federation_id, COALESCE(NEW.amount_msat, 0))
    ON CONFLICT (federation_id) DO UPDATE
        SET wallet_out_msat = federation_totals.wallet_out_msat + COALESCE(NEW.amount_msat, 0);
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

-- Like the v9 triggers, skipped for rows that ON CONFLICT DO NOTHING drops
CREATE TRIGGER transaction_outputs_bump_totals
    AFTER INSERT ON transaction_outputs
    FOR EACH ROW
    WHEN (NEW.kind = 'wallet')
    EXECUTE FUNCTION federation_totals_add_wallet_out();

INSERT INTO federation_totals (federation_id, wallet_in_msat, wallet_out_msat)
SELECT f.federation_id,
       (SELECT COALESCE(SUM(ti.amount_msat), 0)
        FROM transaction_inputs ti
        WHERE ti.federation_id = f.federation_id AND ti.kind = 'wallet'),
       (SELECT COALESCE(SUM(t_o.amount_msat), 0)
        FROM transaction_outputs t_o
        WHERE t_o.federation_id = f.federation_id AND t_o.kind = 'wallet')
FROM federations f
ON CONFLICT (federation_id) DO UPDATE
    SET wallet_in_msat = EXCLUDED.wallet_in_msat,
        wallet_out_msat = EXCLUDED.wallet_out_msat;

-- Hourly guardian health
--
-- The health page averaged the last 30 days of raw samples (one per guardian
-- per minute) on every load. Hours fully inside the window are read from
-- here. Raw samples are kept as they are.

-- Seconds until the next poll of the federation, since fully offline
-- federations are polled less often. Samples from before this migration,
-- when every poll was a minute apart, stay NULL and count as 60.
ALTER TABLE guardian_health ADD COLUMN poll_interval_s INTEGER;

CREATE TABLE guardian_health_hourly (
    federation_id BYTEA NOT NULL REFERENCES federations (federation_id),
    guardian_id INTEGER NOT NULL,
    hour TIMESTAMP NOT NULL,
    samples INTEGER NOT NULL,
    -- Samples where the guardian answered the status request
    online_samples INTEGER NOT NULL,
    latency_samples INTEGER NOT NULL,
    latency_sum_ms BIGINT NOT NULL,
    -- Seconds the samples stand for, and of those the seconds of samples
    -- where the guardian answered: uptime weights samples by their interval
    polled_s BIGINT NOT NULL,
    online_s BIGINT NOT NULL,
    PRIMARY KEY (federation_id, guardian_id, hour)
);

CREATE FUNCTION guardian_health_hourly_add() RETURNS TRIGGER AS $$
BEGIN
    INSERT INTO guardian_health_hourly
    VALUES (
        NEW.federation_id,
        NEW.guardian_id,
        date_trunc('hour', NEW.time),
        1,
        (NEW.status IS NOT NULL)::int,
        (NEW.latency_ms IS NOT NULL)::int,
        COALESCE(NEW.latency_ms, 0),
        COALESCE(NEW.poll_interval_s, 60),
        CASE WHEN NEW.status IS NOT NULL THEN COALESCE(NEW.poll_interval_s, 60) ELSE 0 END
    )
    ON CONFLICT (federation_id, guardian_id, hour) DO UPDATE
        SET samples = guardian_health_hourly.samples + 1,
            online_samples = guardian_health_hourly.online_samples + EXCLUDED.online_samples,
            latency_samples = guardian_health_hourly.latency_samples + EXCLUDED.latency_samples,
            latency_sum_ms = guardian_health_hourly.latency_sum_ms + EXCLUDED.latency_sum_ms,
            polled_s = guardian_health_hourly.polled_s + EXCLUDED.polled_s,
            online_s = guardian_health_hourly.online_s + EXCLUDED.online_s;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER guardian_health_roll_up
    AFTER INSERT ON guardian_health
    FOR EACH ROW EXECUTE FUNCTION guardian_health_hourly_add();

-- Readers only look 30 days back, so older hours are not seeded
INSERT INTO guardian_health_hourly
SELECT federation_id,
       guardian_id,
       date_trunc('hour', time),
       COUNT(*),
       COUNT(status),
       COUNT(latency_ms),
       COALESCE(SUM(latency_ms), 0),
       COUNT(*) * 60,
       COUNT(status) * 60
FROM guardian_health
WHERE time >= date_trunc('hour', NOW()::timestamp - INTERVAL '31 days')
GROUP BY 1, 2, 3;

-- Module schemas
--
-- Each module owns a schema, fmo_<kind>, for the tables it derives from
-- sessions. Core keeps the module-agnostic tables in public. Moving a table is
-- a catalog change: its rows, indexes and constraints stay. Later changes to a
-- module's tables come from ObserverModule::migrations(), run in its schema.
CREATE SCHEMA fmo_wallet;
ALTER TABLE wallet_peg_ins SET SCHEMA fmo_wallet;
ALTER TABLE fmo_wallet.wallet_peg_ins RENAME TO peg_ins;
ALTER TABLE wallet_withdrawal_transactions SET SCHEMA fmo_wallet;
ALTER TABLE fmo_wallet.wallet_withdrawal_transactions RENAME TO withdrawal_transactions;
ALTER TABLE wallet_withdrawal_signatures SET SCHEMA fmo_wallet;
ALTER TABLE fmo_wallet.wallet_withdrawal_signatures RENAME TO withdrawal_signatures;
ALTER TABLE wallet_withdrawal_transaction_inputs SET SCHEMA fmo_wallet;
ALTER TABLE fmo_wallet.wallet_withdrawal_transaction_inputs RENAME TO withdrawal_transaction_inputs;
ALTER TABLE wallet_withdrawal_transaction_outputs SET SCHEMA fmo_wallet;
ALTER TABLE fmo_wallet.wallet_withdrawal_transaction_outputs RENAME TO withdrawal_transaction_outputs;
ALTER MATERIALIZED VIEW utxos SET SCHEMA fmo_wallet;

CREATE SCHEMA fmo_ln;
ALTER TABLE ln_contracts SET SCHEMA fmo_ln;
ALTER TABLE fmo_ln.ln_contracts RENAME TO contracts;
-- The gateway registry and its polls, kept by the Lightning module's task
ALTER TABLE gateways SET SCHEMA fmo_ln;
ALTER TABLE gateway_poll_snapshots SET SCHEMA fmo_ln;

-- block_height_votes stays in public: the wallet module records the votes,
-- but session_times, a core read model, is built from them.

-- How many of its own migrations each module has run. The tables moved above
-- are the starting point every module's migrations build on.
CREATE TABLE module_schema_versions (
    module_kind TEXT PRIMARY KEY,
    migrations_applied INTEGER NOT NULL
);
