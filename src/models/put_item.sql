WITH new_values (store_id, key, value, version) AS (VALUES ($1, $2, $3, $4))
INSERT
INTO vss_db
    (store_id, key, value, version)
SELECT new_values.store_id,
       new_values.key,
       new_values.value,
       new_values.version
FROM new_values
         LEFT JOIN vss_db AS existing ON new_values.store_id = existing.store_id AND new_values.key = existing.key
WHERE CASE
          WHEN new_values.version >= 4294967295 THEN new_values.version >= COALESCE(existing.version, -1)
          ELSE new_values.version > COALESCE(existing.version, -1)
          END
ON CONFLICT (store_id, key)
    DO UPDATE SET value   = excluded.value,
                  version = excluded.version;
