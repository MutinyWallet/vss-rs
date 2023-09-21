use crate::kv::KeyValue;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{BigInt, Bytea, Text};
use diesel_migrations::{embed_migrations, EmbeddedMigrations};
use schema::vss_db;
use serde::{Deserialize, Serialize};

pub mod schema;

pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!();

#[derive(
    QueryableByName,
    Queryable,
    Insertable,
    AsChangeset,
    Serialize,
    Deserialize,
    Debug,
    Clone,
    PartialEq,
)]
#[diesel(check_for_backend(diesel::pg::Pg))]
#[diesel(table_name = vss_db)]
pub struct VssItem {
    pub store_id: String,
    pub key: String,
    pub value: Option<Vec<u8>>,
    pub version: i64,

    created_date: chrono::NaiveDateTime,
    updated_date: chrono::NaiveDateTime,
}

impl VssItem {
    pub fn into_kv(self) -> Option<KeyValue> {
        self.value
            .map(|value| KeyValue::new(self.key, value, self.version))
    }

    pub fn get_item(
        conn: &mut PgConnection,
        store_id: &str,
        key: &str,
    ) -> anyhow::Result<Option<VssItem>> {
        Ok(vss_db::table
            .filter(vss_db::store_id.eq(store_id))
            .filter(vss_db::key.eq(key))
            .first::<Self>(conn)
            .optional()?)
    }

    pub fn put_item(
        conn: &mut PgConnection,
        store_id: &str,
        key: &str,
        value: &[u8],
        version: i64,
    ) -> anyhow::Result<()> {
        sql_query(include_str!("put_item.sql"))
            .bind::<Text, _>(store_id)
            .bind::<Text, _>(key)
            .bind::<Bytea, _>(value)
            .bind::<BigInt, _>(version)
            .execute(conn)?;

        Ok(())
    }

    pub fn list_key_versions(
        conn: &mut PgConnection,
        store_id: &str,
        prefix: Option<&str>,
    ) -> anyhow::Result<Vec<(String, i64)>> {
        let table = vss_db::table
            .filter(vss_db::store_id.eq(store_id))
            .select((vss_db::key, vss_db::version));

        let res = match prefix {
            None => table.load::<(String, i64)>(conn)?,
            Some(prefix) => table
                .filter(vss_db::key.ilike(format!("{prefix}%")))
                .load::<(String, i64)>(conn)?,
        };

        Ok(res)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::State;
    use diesel::{Connection, PgConnection, RunQueryDsl};
    use diesel_migrations::MigrationHarness;
    use secp256k1::Secp256k1;
    use std::str::FromStr;

    const PUBKEY: &str = "04547d92b618856f4eda84a64ec32f1694c9608a3f9dc73e91f08b5daa087260164fbc9e2a563cf4c5ef9f4c614fd9dfca7582f8de429a4799a4b202fbe80a7db5";

    fn init_state() -> State {
        dotenv::dotenv().ok();
        let pg_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
        let mut connection = PgConnection::establish(&pg_url).unwrap();

        connection
            .run_pending_migrations(MIGRATIONS)
            .expect("migrations could not run");

        let auth_key = secp256k1::PublicKey::from_str(PUBKEY).unwrap();

        let secp = Secp256k1::new();

        State {
            pg_url,
            auth_key,
            secp,
        }
    }

    fn clear_database(state: &State) {
        let mut conn = PgConnection::establish(&state.pg_url).unwrap();

        conn.transaction::<_, anyhow::Error, _>(|conn| {
            diesel::delete(vss_db::table).execute(conn)?;
            Ok(())
        })
        .unwrap();
    }

    #[tokio::test]
    async fn test_vss_flow() {
        let state = init_state();
        clear_database(&state);

        let store_id = "test_store_id";
        let key = "test";
        let value = [1, 2, 3, 4, 5];
        let version = 0;

        let mut conn = PgConnection::establish(&state.pg_url).unwrap();
        VssItem::put_item(&mut conn, store_id, key, &value, version).unwrap();

        let versions = VssItem::list_key_versions(&mut conn, store_id, None).unwrap();

        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].0, key);
        assert_eq!(versions[0].1, version);

        let new_value = [6, 7, 8, 9, 10];
        let new_version = version + 1;

        VssItem::put_item(&mut conn, store_id, key, &new_value, new_version).unwrap();

        let item = VssItem::get_item(&mut conn, store_id, key)
            .unwrap()
            .unwrap();

        assert_eq!(item.store_id, store_id);
        assert_eq!(item.key, key);
        assert_eq!(item.value.unwrap(), new_value);
        assert_eq!(item.version, new_version);

        clear_database(&state);
    }

    #[tokio::test]
    async fn test_max_version_number() {
        let state = init_state();
        clear_database(&state);

        let store_id = "max_test_store_id";
        let key = "max_test";
        let value = [1, 2, 3, 4, 5];
        let version = u32::MAX as i64;

        let mut conn = PgConnection::establish(&state.pg_url).unwrap();
        VssItem::put_item(&mut conn, store_id, key, &value, version).unwrap();

        let item = VssItem::get_item(&mut conn, store_id, key)
            .unwrap()
            .unwrap();

        assert_eq!(item.store_id, store_id);
        assert_eq!(item.key, key);
        assert_eq!(item.value.unwrap(), value);

        let new_value = [6, 7, 8, 9, 10];

        VssItem::put_item(&mut conn, store_id, key, &new_value, version).unwrap();

        let item = VssItem::get_item(&mut conn, store_id, key)
            .unwrap()
            .unwrap();

        assert_eq!(item.store_id, store_id);
        assert_eq!(item.key, key);
        assert_eq!(item.value.unwrap(), new_value);

        clear_database(&state);
    }

    #[tokio::test]
    async fn test_list_key_versions() {
        let state = init_state();
        clear_database(&state);

        let store_id = "list_kv_test_store_id";
        let key = "kv_test";
        let key1 = "other_kv_test";
        let value = [1, 2, 3, 4, 5];
        let version = 0;

        let mut conn = PgConnection::establish(&state.pg_url).unwrap();
        VssItem::put_item(&mut conn, store_id, key, &value, version).unwrap();

        VssItem::put_item(&mut conn, store_id, key1, &value, version).unwrap();

        let versions = VssItem::list_key_versions(&mut conn, store_id, None).unwrap();
        assert_eq!(versions.len(), 2);

        let versions = VssItem::list_key_versions(&mut conn, store_id, Some("kv")).unwrap();
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].0, key);
        assert_eq!(versions[0].1, version);

        let versions = VssItem::list_key_versions(&mut conn, store_id, Some("other")).unwrap();
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].0, key1);
        assert_eq!(versions[0].1, version);

        clear_database(&state);
    }
}
