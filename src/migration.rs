use crate::kv::KeyValue;
use crate::models::VssItem;
use crate::State;
use anyhow::anyhow;
use axum::headers::authorization::Bearer;
use axum::headers::Authorization;
use axum::http::StatusCode;
use axum::{Extension, Json, TypedHeader};
use chrono::{DateTime, NaiveDateTime, Utc};
use log::{error, info};
use serde::{Deserialize, Deserializer};
use serde_json::json;
use ureq::Agent;

#[derive(Debug, Clone, Deserialize)]
pub struct Item {
    pub store_id: String,
    pub key: String,
    #[serde(default)]
    pub value: String,
    pub version: i64,

    #[serde(default)]
    #[serde(deserialize_with = "deserialize_datetime_opt")]
    pub created_date: Option<DateTime<Utc>>,

    #[serde(default)]
    #[serde(deserialize_with = "deserialize_datetime_opt")]
    pub updated_date: Option<DateTime<Utc>>,
}

fn deserialize_datetime_opt<'de, D>(deserializer: D) -> Result<Option<DateTime<Utc>>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer).and_then(|opt| {
        if let Some(date_string) = opt {
            let naive = NaiveDateTime::parse_from_str(&date_string, "%Y-%m-%d %H:%M:%S")
                .map_err(serde::de::Error::custom)?;
            #[allow(deprecated)]
            let datetime: DateTime<Utc> = DateTime::from_utc(naive, Utc);
            Ok(Some(datetime))
        } else {
            Ok(None)
        }
    })
}

/// Migration assumes a v1 `get_object` response, which returns value as a base64-encoded string
/// 
/// Environment:
///  - MIGRATION_URL
///  - MIGRATION_BATCH_SIZE (default 100)
///  - MIGRATION_START_INDEX (default 0)
pub async fn migration_impl(admin_key: String, state: &State) -> anyhow::Result<()> {
    let client = Agent::new();
    let Ok(url) = std::env::var("MIGRATION_URL") else {
        return Err(anyhow!("MIGRATION_URL not set"));
    };

    let limit = std::env::var("MIGRATION_BATCH_SIZE")
        .ok()
        .map(|s| s.parse::<usize>())
        .transpose()?
        .unwrap_or(100);

    let mut offset = std::env::var("MIGRATION_START_INDEX")
        .ok()
        .map(|s| s.parse::<usize>())
        .transpose()?
        .unwrap_or(0);

    let mut finished = false;

    info!("Starting migration");
    while !finished {
        info!("Fetching {limit} items from offset {offset}");

        let payload = json!({"limit": limit, "offset": offset});

        let resp = client
            .post(&url)
            .set("x-api-key", &admin_key)
            .send_string(&payload.to_string())?;
        let items: Vec<Item> = resp.into_json()?;
        let nitems = items.len();   // we'll need this later and plan to consume items

        let backend = state.backend.clone();

        // Original migration code didn't preserve timestamps
        // neither does [`VssItem::from_kv`]
        let vss_items: Vec<_> = items.into_iter().filter_map(|item| {
            if let Ok(value) = base64::decode(&item.value) {
                Some(VssItem::from_kv(&item.store_id, KeyValue::new(item.key, value, item.version)))
            } else {
                log::warn!("Failed to decode value during migration: {}", item.value);
                None
            }
        }).collect();

        // Insert values into DB
        backend.put_items(vss_items)?;

        if nitems < limit {
            finished = true;
        } else {
            offset += limit;
        }
    }

    info!("Migration complete!");

    Ok(())
}

pub async fn migration(
    TypedHeader(token): TypedHeader<Authorization<Bearer>>,
    Extension(state): Extension<State>,
) -> Result<Json<()>, (StatusCode, String)> {
    let Ok(admin_key) = std::env::var("ADMIN_KEY") else {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "ADMIN_KEY not set".to_string(),
        ));
    };

    if token.token() != admin_key {
        return Err((StatusCode::UNAUTHORIZED, "Unauthorized".to_string()));
    }

    tokio::spawn(async move {
        if let Err(e) = migration_impl(admin_key, &state).await {
            error!("Migration failed: {e:?}")
        }
    });

    Ok(Json(()))
}
