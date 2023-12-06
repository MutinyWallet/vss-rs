ALTER TABLE vss_db
    ADD COLUMN deleted BOOLEAN NOT NULL DEFAULT FALSE;

-- modify upsert_vss_db to set deleted to false
CREATE OR REPLACE FUNCTION upsert_vss_db(
    p_store_id TEXT,
    p_key TEXT,
    p_value bytea,
    p_version BIGINT
) RETURNS VOID AS
$$
BEGIN

    WITH new_values (store_id, key, value, version) AS (VALUES (p_store_id, p_key, p_value, p_version))
    INSERT
    INTO vss_db
    (store_id, key, value, version)
    SELECT new_values.store_id,
           new_values.key,
           new_values.value,
           new_values.version
    FROM new_values
             LEFT JOIN vss_db AS existing
                       ON new_values.store_id = existing.store_id
                           AND new_values.key = existing.key
    WHERE CASE
              WHEN new_values.version >= 4294967295 THEN new_values.version >= COALESCE(existing.version, -1)
              ELSE new_values.version > COALESCE(existing.version, -1)
              END
    ON CONFLICT (store_id, key)
        DO UPDATE SET value   = excluded.value,
                      version = excluded.version,
                      deleted = false;

END;
$$ LANGUAGE plpgsql;

-- modified upsert_vss_db but to delete
CREATE OR REPLACE FUNCTION delete_item(
    p_store_id TEXT,
    p_key TEXT,
    p_version BIGINT
) RETURNS VOID AS
$$
BEGIN

    WITH new_values (store_id, key, version) AS (VALUES (p_store_id, p_key, p_version))
    INSERT
    INTO vss_db
        (store_id, key, version)
    SELECT new_values.store_id,
           new_values.key,
           new_values.version
    FROM new_values
             LEFT JOIN vss_db AS existing
                       ON new_values.store_id = existing.store_id
                           AND new_values.key = existing.key
    WHERE CASE
              WHEN new_values.version >= 4294967295 THEN new_values.version >= COALESCE(existing.version, -1)
              ELSE new_values.version > COALESCE(existing.version, -1)
              END
    ON CONFLICT (store_id, key)
        DO UPDATE SET value   = NULL,
                      version = excluded.version,
                      deleted = true;

END;
$$ LANGUAGE plpgsql;
