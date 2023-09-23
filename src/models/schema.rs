// @generated automatically by Diesel CLI.

diesel::table! {
    vss_db (store_id, key) {
        store_id -> Text,
        key -> Text,
        value -> Nullable<Bytea>,
        version -> Int8,
        created_date -> Timestamp,
        updated_date -> Timestamp,
    }
}
