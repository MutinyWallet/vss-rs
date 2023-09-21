use crate::auth::verify_token;
use crate::models::VssItem;
use crate::{State, ALLOWED_LOCALHOST, ALLOWED_ORIGINS, ALLOWED_SUBDOMAIN};
use axum::headers::authorization::Bearer;
use axum::headers::{Authorization, Origin};
use axum::http::StatusCode;
use axum::{Extension, Json, TypedHeader};
use diesel::Connection;
use log::{debug, error, trace};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyValue {
    pub key: String,
    pub value: String, // TODO this is supposed to be Vec<u8> to be VSS compatible
    pub version: u64,  // TODO i64 for VSS
}

// TODO nit, change name to "ensure_store_id" since it sets it if it didn't exist
macro_rules! check_store_id {
    ($payload:ident, $store_id:expr) => {
        match $payload.store_id {
            None => $payload.store_id = Some($store_id),
            Some(ref id) => {
                if id != &$store_id {
                    return Err((
                        StatusCode::UNAUTHORIZED,
                        format!("Unauthorized: store_id mismatch"),
                    ));
                }
            }
        }
    };
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetObjectRequest {
    pub store_id: Option<String>,
    pub key: String,
}

pub async fn get_object_impl(
    req: GetObjectRequest,
    state: &State,
) -> anyhow::Result<Option<KeyValue>> {
    let mut conn = state.db_pool.get()?;

    trace!("get_object_impl: {req:?}");
    let store_id = req.store_id.expect("must have");

    let item = VssItem::get_item(&mut conn, &store_id, &req.key)?;

    Ok(item.and_then(|i| i.into_kv()))
}

pub async fn get_object(
    origin: Option<TypedHeader<Origin>>,
    TypedHeader(token): TypedHeader<Authorization<Bearer>>,
    Extension(state): Extension<State>,
    Json(mut payload): Json<GetObjectRequest>,
) -> Result<Json<Option<KeyValue>>, (StatusCode, String)> {
    debug!("get_object: {payload:?}");
    validate_cors(origin)?;
    let store_id = verify_token(token.token(), &state)?;

    check_store_id!(payload, store_id);

    match get_object_impl(payload, &state).await {
        Ok(res) => Ok(Json(res)),
        Err(e) => Err(handle_anyhow_error(e)),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PutObjectsRequest {
    pub store_id: Option<String>,
    pub global_version: Option<u64>,
    pub transaction_items: Vec<KeyValue>,
}

pub async fn put_objects_impl(req: PutObjectsRequest, state: &State) -> anyhow::Result<()> {
    if req.transaction_items.is_empty() {
        return Ok(());
    }

    // todo do something with global version?

    let store_id = req.store_id.expect("must have");

    let mut conn = state.db_pool.get()?;
    conn.transaction(|conn| {
        for kv in req.transaction_items {
            VssItem::put_item(conn, &store_id, &kv.key, &kv.value, kv.version)?;
        }

        Ok(())
    })
}

pub async fn put_objects(
    origin: Option<TypedHeader<Origin>>,
    TypedHeader(token): TypedHeader<Authorization<Bearer>>,
    Extension(state): Extension<State>,
    Json(mut payload): Json<PutObjectsRequest>,
) -> Result<Json<()>, (StatusCode, String)> {
    validate_cors(origin)?;
    let store_id = verify_token(token.token(), &state)?;

    check_store_id!(payload, store_id);

    match put_objects_impl(payload, &state).await {
        Ok(res) => Ok(Json(res)),
        Err(e) => Err(handle_anyhow_error(e)),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListKeyVersionsRequest {
    pub store_id: Option<String>,
    pub key_prefix: Option<String>,
    pub page_size: Option<i32>,
    pub page_token: Option<String>,
}

pub async fn list_key_versions_impl(
    req: ListKeyVersionsRequest,
    state: &State,
) -> anyhow::Result<Vec<Value>> {
    let mut conn = state.db_pool.get()?;

    // todo pagination
    let store_id = req.store_id.expect("must have");

    let versions = VssItem::list_key_versions(&mut conn, &store_id, req.key_prefix.as_deref())?;

    let json = versions
        .into_iter()
        .map(|(key, version)| {
            json!({
                "key": key,
                "version": version,
            })
        })
        .collect();

    Ok(json)
}

pub async fn list_key_versions(
    origin: Option<TypedHeader<Origin>>,
    TypedHeader(token): TypedHeader<Authorization<Bearer>>,
    Extension(state): Extension<State>,
    Json(mut payload): Json<ListKeyVersionsRequest>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    validate_cors(origin)?;
    let store_id = verify_token(token.token(), &state)?;

    check_store_id!(payload, store_id);

    match list_key_versions_impl(payload, &state).await {
        Ok(res) => Ok(Json(res)),
        Err(e) => Err(handle_anyhow_error(e)),
    }
}

pub async fn health_check() -> Result<Json<()>, (StatusCode, String)> {
    Ok(Json(()))
}

fn validate_cors(origin: Option<TypedHeader<Origin>>) -> Result<(), (StatusCode, String)> {
    if let Some(TypedHeader(origin)) = origin {
        if origin.is_null() {
            return Ok(());
        }

        let origin_str = origin.to_string();
        if !ALLOWED_ORIGINS.contains(&origin_str.as_str())
            && !origin_str.ends_with(ALLOWED_SUBDOMAIN)
            && !origin_str.starts_with(ALLOWED_LOCALHOST)
        {
            // The origin is not in the allowed list block the request
            return Err((StatusCode::NOT_FOUND, String::new()));
        }
    }

    Ok(())
}

pub(crate) fn handle_anyhow_error(err: anyhow::Error) -> (StatusCode, String) {
    error!("Error: {err:?}");
    (StatusCode::BAD_REQUEST, format!("{err}"))
}
