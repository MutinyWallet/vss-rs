/// Selectable backend datastore
/// 
/// Implementation Status
///  - [X] Postgres
///  - [ ] Sqlite
///  - [ ] Rocksdb
use crate::kv::KeyValue;
use crate::models::VssItem;
use diesel::prelude::*;
use diesel::r2d2::PooledConnection;
use diesel::r2d2::{ConnectionManager, Pool};
use diesel::sql_query;
use diesel::sql_types::{BigInt, Bytea, Text};
use diesel::PgConnection;

/// For introspection on dyn trait object (since const generics for ADT still in nightly)
pub enum BackendType {
    Postgres,
    Rocksdb,
}

pub struct Postgres {
    db_pool: Pool<ConnectionManager<PgConnection>>,
}

impl Postgres {
    pub fn new(db_pool: Pool<ConnectionManager<PgConnection>>) -> Self {
        Self { db_pool }
    }
    fn conn(
        &self,
    ) -> anyhow::Result<PooledConnection<ConnectionManager<diesel::PgConnection>>, anyhow::Error>
    {
        Ok(self.db_pool.get()?)
    }
}

pub struct Rocksdb;

pub trait VssBackend: Send + Sync {
    fn backend_type(&self) -> BackendType;
    fn get_item(&self, store_id: &str, key: &str) -> anyhow::Result<Option<VssItem>>;
    
    #[cfg(test)]
    /// This method fetches its own connection even if called from within a tx,
    /// so it is moved to cfg(test) to prevent errors. It is manually inlined
    /// into [`VssBackend::put_items`] and [`VssBackend::put_items_in_store`]
    fn put_item(&self, store_id: &str, key: &str, value: &[u8], version: i64)
        -> anyhow::Result<()>;

    /// Wrap multiple item writes in a transaction
    /// Takes `Vec<[VssItem]>`, ∴ Items may go to different `store_id`s
    /// If you need atomic put for items all with same `store_id`, see [`VssBackend::put_items_in_store`]
    fn put_items(&self, items: Vec<VssItem>) -> anyhow::Result<()>;

    /// Wrap multiple item writes in a transaction
    /// Items all placed in same `store_id`
    /// If you need atomic put to potentially different store_ids, see [`VssBackend::put_items`]
    fn put_items_in_store(&self, store_id: &str, items: Vec<KeyValue>) -> anyhow::Result<()>;

    fn list_key_versions(
        &self,
        store_id: &str,
        prefix: Option<&str>,
    ) -> anyhow::Result<Vec<(String, i64)>>;

    #[cfg(test)]
    /// THIS WILL NUKE YOUR DATA
    /// Make sure `DATABASE_URL`` is not same as your dev/stage/prod connection
    /// when running `cargo test`
    fn clear_database(&self);
}

impl VssBackend for Postgres {
    fn backend_type(&self) -> BackendType {
        BackendType::Postgres
    }

    fn get_item(&self, store_id: &str, key: &str) -> anyhow::Result<Option<VssItem>> {
        use super::schema::vss_db;

        let mut conn = self.conn()?;

        Ok(vss_db::table
            .filter(vss_db::store_id.eq(store_id))
            .filter(vss_db::key.eq(key))
            .first::<VssItem>(&mut conn)
            .optional()?)
    }

    #[cfg(test)]
    fn put_item(
        &self,
        store_id: &str,
        key: &str,
        value: &[u8],
        version: i64,
    ) -> anyhow::Result<()> {
        let mut conn = self.conn()?;

        sql_query("SELECT upsert_vss_db($1, $2, $3, $4)")
            .bind::<Text, _>(store_id)
            .bind::<Text, _>(key)
            .bind::<Bytea, _>(value)
            .bind::<BigInt, _>(version)
            .execute(&mut conn)?;

        Ok(())
    }

    fn put_items(&self, items: Vec<VssItem>) -> anyhow::Result<()> {
        let mut conn = self.conn()?;

        conn.transaction::<_, anyhow::Error, _>(|conn| {
            for item in items {
                // VssItem.value is Option for unclear reasons
                let value = match item.value {
                    None => vec![],
                    Some(v) => v,
                };
                // Inline VssBackend::put_item which was moved to #[cfg(test)] only
                sql_query("SELECT upsert_vss_db($1, $2, $3, $4)")
                    .bind::<Text, _>(item.store_id)
                    .bind::<Text, _>(item.key)
                    .bind::<Bytea, _>(value)
                    .bind::<BigInt, _>(item.version)
                    .execute(conn)?;                    
            }
    
            Ok(())
        })?;

        Ok(())
    }

    fn put_items_in_store(&self, store_id: &str, items: Vec<KeyValue>) -> anyhow::Result<()> {
        let mut conn = self.conn()?;

        conn.transaction::<_, anyhow::Error, _>(|conn| {
            for kv in items {
                // Inline VssBackend::put_item which was moved to #[cfg(test)] only
                sql_query("SELECT upsert_vss_db($1, $2, $3, $4)")
                    .bind::<Text, _>(store_id)
                    .bind::<Text, _>(kv.key)
                    .bind::<Bytea, _>(kv.value.0)
                    .bind::<BigInt, _>(kv.version)
                    .execute(conn)?;                    
            }
    
            Ok(())
        })?;

        Ok(())
    }

    fn list_key_versions(
        &self,
        store_id: &str,
        prefix: Option<&str>,
    ) -> anyhow::Result<Vec<(String, i64)>> {
        use super::schema::vss_db;

        let mut conn = self.conn()?;

        let table = vss_db::table
            .filter(vss_db::store_id.eq(store_id))
            .select((vss_db::key, vss_db::version));

        let res = match prefix {
            None => table.load::<(String, i64)>(&mut conn)?,
            Some(prefix) => table
                .filter(vss_db::key.ilike(format!("{prefix}%")))
                .load::<(String, i64)>(&mut conn)?,
        };

        Ok(res)
    }

    #[cfg(test)]
    fn clear_database(&self) {
        use crate::models::schema::vss_db;

        let mut conn = self.conn().unwrap();
        conn.transaction::<_, anyhow::Error, _>(|conn| {
            diesel::delete(vss_db::table).execute(conn)?;
            Ok(())
        })
        .unwrap();
    }
}

impl VssBackend for Rocksdb {
    fn backend_type(&self) -> BackendType {
        BackendType::Rocksdb
    }

    fn get_item(&self, _store_id: &str, _key: &str) -> anyhow::Result<Option<VssItem>> {
        todo!();
        //Ok(None)
    }

    #[cfg(test)]
    fn put_item(
        &self,
        _store_id: &str,
        _key: &str,
        _value: &[u8],
        _version: i64,
    ) -> anyhow::Result<()> {
        todo!();
    }

    fn put_items(&self, _items: Vec<VssItem>) -> anyhow::Result<()> {
        todo!();
        //Ok(())
    }

    fn put_items_in_store(&self, _store_id: &str, _items: Vec<KeyValue>) -> anyhow::Result<()> {
        todo!();
        //Ok(())
    }

    fn list_key_versions(
        &self,
        _store_id: &str,
        _prefix: Option<&str>,
    ) -> anyhow::Result<Vec<(String, i64)>> {
        todo!();
        //Ok(vec![])
    }

    #[cfg(test)]
    fn clear_database(&self) {
        todo!();
    }
}
