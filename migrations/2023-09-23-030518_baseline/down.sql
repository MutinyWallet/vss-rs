-- 1. Drop the triggers
DROP TRIGGER IF EXISTS tr_set_dates_after_insert ON vss_db;
DROP TRIGGER IF EXISTS tr_set_dates_after_update ON vss_db;

-- 2. Drop the trigger functions
DROP FUNCTION IF EXISTS set_created_date();
DROP FUNCTION IF EXISTS set_updated_date();

DROP TABLE IF EXISTS vss_db;