-- Create the update session_times view
BEGIN;
INSERT INTO schema_version (version)
VALUES (6);
