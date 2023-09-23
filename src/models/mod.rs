use crate::kv::KeyValue;
use serde::{Deserialize, Serialize};
use tokio_postgres::{Client, Row};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
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

    pub async fn get_item(
        client: &Client,
        store_id: &String,
        key: &String,
    ) -> anyhow::Result<Option<VssItem>> {
        client
            .query_opt(
                "SELECT * FROM vss_db WHERE store_id = $1 AND key = $2",
                &[store_id, key],
            )
            .await?
            .map(row_to_vss_item)
            .transpose()
    }

    pub async fn put_item(
        client: &Client,
        store_id: &String,
        key: &String,
        value: &Vec<u8>,
        version: i64,
    ) -> anyhow::Result<()> {
        client
            .execute(
                "SELECT upsert_vss_db($1, $2, $3, $4)",
                &[store_id, key, value, &version],
            )
            .await?;

        Ok(())
    }

    pub async fn list_key_versions(
        client: &Client,
        store_id: &String,
        prefix: Option<&String>,
    ) -> anyhow::Result<Vec<(String, i64)>> {
        let rows = match prefix {
            Some(prefix) => client
                .query(
                    "SELECT key, version FROM vss_db WHERE store_id = $1 AND key ILIKE $2 || '%'",
                    &[store_id, prefix],
                )
                .await?,
            None => {
                client
                    .query(
                        "SELECT key, version FROM vss_db WHERE store_id = $1",
                        &[store_id],
                    )
                    .await?
            }
        };

        let res = rows
            .into_iter()
            .map(|row| {
                let key: String = row.get(0);
                let version: i64 = row.get(1);

                (key, version)
            })
            .collect();

        Ok(res)
    }
}

fn row_to_vss_item(row: Row) -> anyhow::Result<VssItem> {
    let store_id: String = row.get(0);
    let key: String = row.get(1);
    let value: Option<Vec<u8>> = row.get(2);
    let version: i64 = row.get(3);
    let created_date: chrono::NaiveDateTime = row.get(4);
    let updated_date: chrono::NaiveDateTime = row.get(5);

    Ok(VssItem {
        store_id,
        key,
        value,
        version,
        created_date,
        updated_date,
    })
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::State;
    use secp256k1::Secp256k1;
    use std::str::FromStr;
    use std::sync::Arc;
    use tokio_postgres::{Config, NoTls};

    const PUBKEY: &str = "04547d92b618856f4eda84a64ec32f1694c9608a3f9dc73e91f08b5daa087260164fbc9e2a563cf4c5ef9f4c614fd9dfca7582f8de429a4799a4b202fbe80a7db5";

    async fn init_state() -> State {
        dotenv::dotenv().ok();
        let pg_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
        let mut config = Config::from_str(&pg_url).unwrap();
        config.pgbouncer_mode(true);
        // Connect to the database.
        let (client, connection) = config.connect(NoTls).await.unwrap();

        // The connection object performs the actual communication with the database,
        // so spawn it off to run on its own.
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("db connection error: {e}");
            }
        });

        client
            .simple_query("DROP TABLE IF EXISTS vss_db")
            .await
            .unwrap();
        client
            .simple_query(include_str!("migration_baseline.sql"))
            .await
            .unwrap();

        let auth_key = secp256k1::PublicKey::from_str(PUBKEY).unwrap();

        let secp = Secp256k1::new();

        State {
            client: Arc::new(client),
            auth_key,
            secp,
        }
    }

    async fn clear_database(state: &State) {
        state
            .client
            .simple_query("DROP TABLE vss_db")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_vss_flow() {
        let state = init_state().await;

        let store_id = "test_store_id".to_string();
        let key = "test".to_string();
        let value: Vec<u8> = vec![1, 2, 3, 4, 5];
        let version = 0;

        VssItem::put_item(&state.client, &store_id, &key, &value, version)
            .await
            .unwrap();

        let versions = VssItem::list_key_versions(&state.client, &store_id, None)
            .await
            .unwrap();

        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].0, key);
        assert_eq!(versions[0].1, version);

        let new_value = vec![6, 7, 8, 9, 10];
        let new_version = version + 1;

        VssItem::put_item(&state.client, &store_id, &key, &new_value, new_version)
            .await
            .unwrap();

        let item = VssItem::get_item(&state.client, &store_id, &key)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(item.store_id, store_id);
        assert_eq!(item.key, key);
        assert_eq!(item.value.unwrap(), new_value);
        assert_eq!(item.version, new_version);

        clear_database(&state).await;
    }

    #[tokio::test]
    async fn test_max_version_number() {
        let state = init_state().await;

        let store_id = "max_test_store_id".to_string();
        let key = "max_test".to_string();
        let value = vec![1, 2, 3, 4, 5];
        let version = u32::MAX as i64;

        VssItem::put_item(&state.client, &store_id, &key, &value, version)
            .await
            .unwrap();

        let item = VssItem::get_item(&state.client, &store_id, &key)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(item.store_id, store_id);
        assert_eq!(item.key, key);
        assert_eq!(item.value.unwrap(), value);

        let new_value = vec![6, 7, 8, 9, 10];

        VssItem::put_item(&state.client, &store_id, &key, &new_value, version)
            .await
            .unwrap();

        let item = VssItem::get_item(&state.client, &store_id, &key)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(item.store_id, store_id);
        assert_eq!(item.key, key);
        assert_eq!(item.value.unwrap(), new_value);

        clear_database(&state).await;
    }

    #[tokio::test]
    async fn test_list_key_versions() {
        let state = init_state().await;

        let store_id = "list_kv_test_store_id".to_string();
        let key = "kv_test".to_string();
        let key1 = "other_kv_test".to_string();
        let value = vec![1, 2, 3, 4, 5];
        let version = 0;

        VssItem::put_item(&state.client, &store_id, &key, &value, version)
            .await
            .unwrap();

        VssItem::put_item(&state.client, &store_id, &key1, &value, version)
            .await
            .unwrap();

        let versions = VssItem::list_key_versions(&state.client, &store_id, None)
            .await
            .unwrap();
        assert_eq!(versions.len(), 2);

        let versions =
            VssItem::list_key_versions(&state.client, &store_id, Some(&"kv".to_string()))
                .await
                .unwrap();
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].0, key);
        assert_eq!(versions[0].1, version);

        let versions =
            VssItem::list_key_versions(&state.client, &store_id, Some(&"other".to_string()))
                .await
                .unwrap();
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].0, key1);
        assert_eq!(versions[0].1, version);

        clear_database(&state).await;
    }
}
