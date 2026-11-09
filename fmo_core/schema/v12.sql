-- Read models kept up to date as sessions and health samples are written, so
-- page loads and background jobs stop aggregating over the full history; module
-- schemas; repair jobs for session times; and removal of unused objects

INSERT INTO schema_version (version)
VALUES (12);

-- Block writers until the seeds below are done and the maintenance below
-- takes over, so no row is counted twice or missed
LOCK TABLE sessions, block_height_votes, transactions, transaction_inputs,
    transaction_outputs, guardian_health IN SHARE MODE;

-- Session times
--
-- The materialized view recomputed every session of every federation each
-- minute. A session's time only depends on its own votes and the sessions
-- before it, and sessions are committed in order, so it is final once written.
DROP MATERIALIZED VIEW session_times;

CREATE TABLE session_times (
    federation_id BYTEA NOT NULL,
    session_index INTEGER NOT NULL,
    -- The height most guardians voted for in the session, NULL without votes
    block_height INTEGER,
    -- The latest time voted for in this session or an earlier one: the
    -- median of the session's unix time votes, or without those the time of
    -- the block its height votes agree on
    estimated_session_timestamp TIMESTAMP,
    PRIMARY KEY (federation_id, session_index)
);

-- Unix time votes, in seconds. Modules that agree on the time, such as
-- Lightning v2 to the minute, date sessions more precisely than block heights
-- can, and federations without a v1 wallet module have no height votes.
CREATE TABLE unix_time_votes (
    federation_id BYTEA   NOT NULL REFERENCES federations (federation_id),
    session_index INTEGER NOT NULL,
    item_index    INTEGER NOT NULL,
    proposer      INTEGER NOT NULL,
    time_vote     BIGINT  NOT NULL,
    PRIMARY KEY (federation_id, session_index, item_index),
    FOREIGN KEY (federation_id, session_index) REFERENCES sessions (federation_id, session_index)
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
-- higher one instead of an arbitrary one, and that the median of a session's
-- unix time votes, where it has some, dates it instead of the block time
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
    proposer_times AS (
        SELECT session_index, proposer, MAX(time_vote) AS time_vote
        FROM unix_time_votes
        WHERE federation_id = fed AND session_index BETWEEN from_index AND to_index
        GROUP BY session_index, proposer
    ),
    session_voted_times AS (
        SELECT session_index,
               to_timestamp(percentile_disc(0.5) WITHIN GROUP (ORDER BY time_vote))::timestamp AS time
        FROM proposer_times
        GROUP BY session_index
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
               MAX(COALESCE(svt.time, bt.timestamp))
                   OVER (ORDER BY s.session_index ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW),
               (SELECT estimated_session_timestamp FROM previous)
           )
    FROM sessions s
    LEFT JOIN session_heights sh ON sh.session_index = s.session_index
    LEFT JOIN block_times bt ON bt.block_height = sh.height
    LEFT JOIN session_voted_times svt ON svt.session_index = s.session_index
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
-- Sessions without a time yet (before their federation's first vote) are left
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
CREATE INDEX federation_daily_activity_day ON federation_daily_activity(day);

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

INSERT INTO federation_daily_activity (federation_id, day, tx_count, funded_tx_count, volume_msat)
SELECT f.federation_id, a.day, a.tx_count, a.funded_tx_count, a.volume_msat
FROM federations f, session_range_activity(f.federation_id, 0, 2147483647) a;

-- Called by the sync for every session it writes, in the session's
-- transaction. Counts the session's transactions on top of what is there, so
-- it must run exactly once per session: after stored sessions are
-- reprocessed, schedule_read_model_rebuild asks for a rebuild instead.
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

-- The first day a session from `from_index` on is dated to
CREATE FUNCTION session_activity_floor(fed BYTEA, from_index INTEGER) RETURNS DATE AS $$
    SELECT DATE(MIN(estimated_session_timestamp))
    FROM session_times
    WHERE federation_id = fed AND session_index >= from_index;
$$ LANGUAGE sql STABLE;

-- Recounts the federation's activity on the days from `from_day` up to, not
-- including, `to_day` (NULL: every later day). The collector rebuilds read
-- models in steps of bounded size, each in its own transaction, so the
-- federation's ingest, which needs the read-model lock for every session,
-- waits for one step at most (FederationObserver::rebuild_read_models).
CREATE FUNCTION rebuild_daily_activity_days(fed BYTEA, from_day DATE, to_day DATE)
RETURNS VOID AS $$
DECLARE
    first_session INTEGER;
    last_session INTEGER;
BEGIN
    PERFORM lock_federation_read_models(fed);
    DELETE FROM federation_daily_activity
    WHERE federation_id = fed AND day >= from_day AND (to_day IS NULL OR day < to_day);
    SELECT MIN(session_index), MAX(session_index) INTO first_session, last_session
    FROM session_times
    WHERE federation_id = fed
      AND estimated_session_timestamp >= from_day
      AND (to_day IS NULL OR estimated_session_timestamp < to_day);
    IF first_session IS NULL THEN RETURN; END IF;
    INSERT INTO federation_daily_activity (federation_id, day, tx_count, funded_tx_count, volume_msat)
    SELECT fed, a.day, a.tx_count, a.funded_tx_count, a.volume_msat
    FROM session_range_activity(fed, first_session, last_session) a
    WHERE a.day >= from_day AND (to_day IS NULL OR a.day < to_day);
END;
$$ LANGUAGE plpgsql;

-- Session time repairs
--
-- Votes must remain ingestible while the block explorer is behind, so the
-- foreign key to block_times goes, and a vote for a block without a time yet
-- leaves a durable job to rebuild from its session once the time arrives.
-- Until now that key held, so every stored vote's block has a time and the
-- seed above dated every session that has votes: there is nothing to repair.
-- block_height -1 is a job that waits for no block: a rebuild after stored
-- sessions were reprocessed.
DO $$
DECLARE constraint_name TEXT;
BEGIN
    FOR constraint_name IN
        SELECT conname FROM pg_constraint
        WHERE conrelid = 'block_height_votes'::regclass
          AND confrelid = 'block_times'::regclass AND contype = 'f'
    LOOP
        EXECUTE format('ALTER TABLE block_height_votes DROP CONSTRAINT %I', constraint_name);
    END LOOP;
END;
$$;
CREATE TABLE session_time_repairs (
    federation_id BYTEA NOT NULL REFERENCES federations (federation_id),
    block_height INTEGER NOT NULL,
    from_index INTEGER NOT NULL,
    -- When the collector last claimed the job; claims go to the least recently
    -- attempted federation, so one that keeps failing holds back no other
    attempted_at TIMESTAMPTZ,
    PRIMARY KEY (federation_id, block_height)
);
CREATE INDEX session_time_repairs_height ON session_time_repairs (block_height);

CREATE FUNCTION schedule_missing_block_time_repair() RETURNS trigger AS $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM block_times WHERE block_height = NEW.height_vote) THEN
        INSERT INTO session_time_repairs (federation_id, block_height, from_index)
        VALUES (NEW.federation_id, NEW.height_vote, NEW.session_index)
        ON CONFLICT (federation_id, block_height) DO UPDATE
            SET from_index = LEAST(session_time_repairs.from_index, EXCLUDED.from_index);
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER schedule_missing_block_time_repair
    AFTER INSERT ON block_height_votes
    FOR EACH ROW EXECUTE FUNCTION schedule_missing_block_time_repair();

-- Marks the ready jobs of the least recently attempted federation as claimed
-- at `claimed_at`, and returns the session to rebuild from. NULLs when no job
-- is ready.
CREATE FUNCTION claim_session_time_repair(OUT fed BYTEA, OUT start_index INTEGER, OUT claimed_at TIMESTAMPTZ)
AS $$
BEGIN
    SELECT r.federation_id INTO fed FROM session_time_repairs r
    WHERE r.block_height = -1 OR EXISTS
        (SELECT 1 FROM block_times b WHERE b.block_height = r.block_height)
    ORDER BY r.attempted_at NULLS FIRST, r.block_height, r.federation_id
    LIMIT 1;
    IF fed IS NULL THEN RETURN; END IF;
    claimed_at := clock_timestamp();
    UPDATE session_time_repairs r SET attempted_at = claimed_at
    WHERE r.federation_id = fed AND (r.block_height = -1 OR EXISTS
        (SELECT 1 FROM block_times b WHERE b.block_height = r.block_height));
    SELECT MIN(r.from_index) INTO start_index FROM session_time_repairs r
    WHERE r.federation_id = fed AND r.attempted_at = claimed_at;
END;
$$ LANGUAGE plpgsql;

-- Deletes the jobs a claim covered once its rebuild is done. A job moved to an
-- earlier session, or scheduled again, since the claim stays.
CREATE FUNCTION finish_session_time_repair(fed BYTEA, start_index INTEGER, claimed_at TIMESTAMPTZ)
RETURNS VOID AS $$
    DELETE FROM session_time_repairs
    WHERE federation_id = fed AND attempted_at = claimed_at AND from_index >= start_index;
$$ LANGUAGE sql;

-- Asks the collector to rebuild the federation's session times and daily
-- activity from `from_index`, after stored sessions were reprocessed. Durable,
-- and merged with the federation's other rebuilds.
CREATE FUNCTION schedule_read_model_rebuild(fed BYTEA, from_index INTEGER) RETURNS VOID AS $$
    INSERT INTO session_time_repairs (federation_id, block_height, from_index)
    VALUES (fed, -1, from_index)
    ON CONFLICT (federation_id, block_height) DO UPDATE
        SET from_index = LEAST(session_time_repairs.from_index, EXCLUDED.from_index),
            attempted_at = NULL;
$$ LANGUAGE sql;

-- BEGIN WORK NOTIFICATIONS
-- (fmo_core/src/work/tests.rs runs this part on its own)
-- Delivered after commit. Startup also drains work created while offline.
CREATE FUNCTION notify_session_time_repair() RETURNS trigger AS $$
BEGIN
    PERFORM pg_notify('fmo_work', 'session-time-repair');
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER notify_session_time_repair
    AFTER INSERT OR UPDATE ON session_time_repairs
    FOR EACH STATEMENT EXECUTE FUNCTION notify_session_time_repair();
CREATE TRIGGER notify_repair_block_arrival
    AFTER INSERT ON block_times
    FOR EACH STATEMENT EXECUTE FUNCTION notify_session_time_repair();
-- END WORK NOTIFICATIONS

-- Session counts
CREATE TABLE federation_session_counts (
    federation_id BYTEA PRIMARY KEY REFERENCES federations(federation_id),
    session_count BIGINT NOT NULL
);
INSERT INTO federation_session_counts
SELECT federation_id, COUNT(*) FROM sessions GROUP BY federation_id;
CREATE FUNCTION count_stored_session() RETURNS TRIGGER AS $$
BEGIN
    INSERT INTO federation_session_counts VALUES (NEW.federation_id, 1)
    ON CONFLICT (federation_id) DO UPDATE
        SET session_count = federation_session_counts.session_count + 1;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER sessions_count_insert AFTER INSERT ON sessions
FOR EACH ROW EXECUTE FUNCTION count_stored_session();

-- Federation assets
--
-- The federation list summed every wallet input and output of each
-- federation on every load. Kept by the v9 counters instead.
--
-- federation_totals has one row per federation. Updated as each transaction,
-- input or output was written, that row stayed locked until the writer
-- committed: a projector completing amounts held it for its whole batch, and
-- the federation's ingest, which needs it for every session, ran into its lock
-- timeout. Deferred to commit, the counters change at the end of the writing
-- transaction and the row is locked only while it commits. Writers that also
-- take the federation's read-model lock (record_new_session, deposit
-- enrichment) all take it before the totals row, in one order.
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

-- A module added to a build later fills in the amounts of its items, which
-- ingest stored without them. Keep the counters right for those updates too,
-- as the insert triggers do for new rows.
CREATE FUNCTION federation_totals_update_volume() RETURNS TRIGGER AS $$
DECLARE
    delta BIGINT := COALESCE(NEW.amount_msat, 0) - COALESCE(OLD.amount_msat, 0);
    wallet_delta BIGINT := CASE WHEN NEW.kind = 'wallet' THEN delta ELSE 0 END;
BEGIN
    INSERT INTO federation_totals (federation_id, tx_volume_msat, wallet_in_msat)
    VALUES (NEW.federation_id, delta, wallet_delta)
    ON CONFLICT (federation_id) DO UPDATE
        SET tx_volume_msat = federation_totals.tx_volume_msat + delta,
            wallet_in_msat = federation_totals.wallet_in_msat + wallet_delta;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE FUNCTION federation_totals_update_wallet_out() RETURNS TRIGGER AS $$
DECLARE
    delta BIGINT := COALESCE(NEW.amount_msat, 0) - COALESCE(OLD.amount_msat, 0);
BEGIN
    INSERT INTO federation_totals (federation_id, wallet_out_msat)
    VALUES (NEW.federation_id, delta)
    ON CONFLICT (federation_id) DO UPDATE
        SET wallet_out_msat = federation_totals.wallet_out_msat + delta;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

-- The v9 insert counters, deferred; the rest new. Like the v9 triggers, they
-- skip rows that ON CONFLICT DO NOTHING drops.
DROP TRIGGER transactions_bump_totals ON transactions;
CREATE CONSTRAINT TRIGGER transactions_bump_totals
    AFTER INSERT ON transactions
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION federation_totals_count_tx();

DROP TRIGGER transaction_inputs_bump_totals ON transaction_inputs;
CREATE CONSTRAINT TRIGGER transaction_inputs_bump_totals
    AFTER INSERT ON transaction_inputs
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION federation_totals_add_volume();

CREATE CONSTRAINT TRIGGER transaction_outputs_bump_totals
    AFTER INSERT ON transaction_outputs
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW
    WHEN (NEW.kind = 'wallet')
    EXECUTE FUNCTION federation_totals_add_wallet_out();

CREATE CONSTRAINT TRIGGER transaction_inputs_update_totals
    AFTER UPDATE OF amount_msat ON transaction_inputs
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW
    WHEN (OLD.amount_msat IS DISTINCT FROM NEW.amount_msat)
    EXECUTE FUNCTION federation_totals_update_volume();

CREATE CONSTRAINT TRIGGER transaction_outputs_update_totals
    AFTER UPDATE OF amount_msat ON transaction_outputs
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW
    WHEN (NEW.kind = 'wallet' AND OLD.amount_msat IS DISTINCT FROM NEW.amount_msat)
    EXECUTE FUNCTION federation_totals_update_wallet_out();

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

CREATE INDEX guardian_health_retention_time ON guardian_health(time);

-- Observations
--
-- Request handlers read observations; background workers own remote I/O.
CREATE TABLE federation_observations (
    federation_id BYTEA NOT NULL REFERENCES federations(federation_id),
    kind TEXT NOT NULL,
    data JSONB NOT NULL,
    observed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (federation_id, kind)
);

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

-- block_height_votes and unix_time_votes stay in public: modules cast the
-- votes, but session_times, a core read model, is built from them.

-- How many of its own migrations each module has run. The tables moved above
-- are the starting point every module's migrations build on.
CREATE TABLE module_schema_versions (
    module_kind TEXT PRIMARY KEY,
    migrations_applied INTEGER NOT NULL
);

-- Where each module's projector is for each federation: the next stored
-- session it processes. Modules derive their tables from the stored sessions
-- behind core's ingest, each on its own cursor and in its own transactions. A
-- module without a row starts at session 0 when it is registered.
CREATE TABLE module_cursors (
    module_kind   TEXT        NOT NULL,
    federation_id BYTEA       NOT NULL REFERENCES federations (federation_id),
    next_session  INTEGER     NOT NULL,
    -- The first session whose core rows the projector completed and whose
    -- session times and daily activity wait for a rebuild, done once it has
    -- caught up
    repair_from   INTEGER,
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (module_kind, federation_id)
);

-- Before v12 every session was processed by the mint, wallet and ln modules in
-- the transaction that stored it, by a build that had to include the three, so
-- they are caught up with every stored session.
INSERT INTO module_cursors (module_kind, federation_id, next_session)
SELECT k.kind, s.federation_id, MAX(s.session_index) + 1
FROM sessions s
         CROSS JOIN (VALUES ('mint'), ('wallet'), ('ln')) AS k(kind)
GROUP BY k.kind, s.federation_id;

-- Unused objects
--
-- Never read
DROP VIEW latest_guardian_health;
-- Backed the foreign key to block_times dropped above; nothing looks votes up
-- by height
DROP INDEX block_height_vote_heights;
-- Nothing reads votes by fetch time
DROP INDEX nostr_votes_fetch_time;
-- Leading columns of their table's primary key, whose index serves the same
-- lookups
DROP INDEX federation_sessions;
DROP INDEX federation_transactions;
DROP INDEX federation_inputs;
DROP INDEX federation_transaction_inputs;
DROP INDEX federation_outputs;
DROP INDEX federation_transaction_outputs;
DROP INDEX block_height_vote_federation_sessions;
DROP INDEX federation_consensus_items;
DROP INDEX federation_session_consensus_items;
DROP INDEX federation_input_details;
DROP INDEX federation_transaction_input_details;
DROP INDEX federation_output_details;
DROP INDEX federation_transaction_output_details;
