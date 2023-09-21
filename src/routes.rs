use crate::auth::verify_token;
use crate::kv::{KeyValue, KeyValueOld};
use crate::models::VssItem;
use crate::{State, ALLOWED_LOCALHOST, ALLOWED_ORIGINS, ALLOWED_SUBDOMAIN};
use axum::headers::authorization::Bearer;
use axum::headers::{Authorization, HeaderMap, Origin};
use axum::http::header::{
    ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS, ACCESS_CONTROL_ALLOW_ORIGIN,
    CONTENT_TYPE,
};
use axum::http::StatusCode;
use axum::{Extension, Json, TypedHeader};
use diesel::{Connection, PgConnection};
use log::{debug, error, trace};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

macro_rules! ensure_store_id {
    ($payload:ident, $store_id:expr, $headers:ident) => {
        match $payload.store_id {
            None => $payload.store_id = Some($store_id),
            Some(ref id) => {
                if id != &$store_id {
                    return Err((
                        StatusCode::UNAUTHORIZED,
                        $headers,
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
    trace!("get_object_impl: {req:?}");
    let store_id = req.store_id.expect("must have");

    let mut conn = PgConnection::establish(&state.pg_url).unwrap();
    let item = VssItem::get_item(&mut conn, &store_id, &req.key)?;

    Ok(item.and_then(|i| i.into_kv()))
}

pub async fn get_object(
    origin: Option<TypedHeader<Origin>>,
    TypedHeader(token): TypedHeader<Authorization<Bearer>>,
    Extension(state): Extension<State>,
    Json(mut payload): Json<GetObjectRequest>,
) -> Result<(HeaderMap, Json<Option<KeyValueOld>>), (StatusCode, HeaderMap, String)> {
    debug!("get_object: {payload:?}");
    let origin = validate_cors(origin)?;
    let mut headers = create_cors_headers(&origin);
    headers.insert(CONTENT_TYPE, "application/json".parse().unwrap());

    let store_id = verify_token(token.token(), &state, &headers)?;

    ensure_store_id!(payload, store_id, headers);

    match get_object_impl(payload, &state).await {
        Ok(Some(res)) => Ok((headers, Json(Some(res.into())))),
        Ok(None) => Ok((headers, Json(None))),
        Err(e) => Err(handle_anyhow_error(e, headers)),
    }
}

pub async fn get_object_v2(
    origin: Option<TypedHeader<Origin>>,
    TypedHeader(token): TypedHeader<Authorization<Bearer>>,
    Extension(state): Extension<State>,
    Json(mut payload): Json<GetObjectRequest>,
) -> Result<Json<Option<KeyValue>>, (StatusCode, HeaderMap, String)> {
    debug!("get_object v2: {payload:?}");
    let origin = validate_cors(origin)?;
    let mut headers = create_cors_headers(&origin);
    headers.insert(CONTENT_TYPE, "application/json".parse().unwrap());

    let store_id = verify_token(token.token(), &state, &headers)?;

    ensure_store_id!(payload, store_id, headers);

    match get_object_impl(payload, &state).await {
        Ok(res) => Ok(Json(res)),
        Err(e) => Err(handle_anyhow_error(e, headers)),
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

    let mut conn = PgConnection::establish(&state.pg_url).unwrap();
    conn.transaction(|conn| {
        for kv in req.transaction_items {
            VssItem::put_item(conn, &store_id, &kv.key, &kv.value.0, kv.version)?;
        }

        Ok(())
    })
}

pub async fn put_objects(
    origin: Option<TypedHeader<Origin>>,
    TypedHeader(token): TypedHeader<Authorization<Bearer>>,
    Extension(state): Extension<State>,
    Json(mut payload): Json<PutObjectsRequest>,
) -> Result<(HeaderMap, Json<()>), (StatusCode, HeaderMap, String)> {
    let origin = validate_cors(origin)?;
    let mut headers = create_cors_headers(&origin);
    headers.insert(CONTENT_TYPE, "application/json".parse().unwrap());

    let store_id = verify_token(token.token(), &state, &headers)?;

    ensure_store_id!(payload, store_id, headers);

    match put_objects_impl(payload, &state).await {
        Ok(res) => Ok((headers, Json(res))),
        Err(e) => Err(handle_anyhow_error(e, headers)),
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
    // todo pagination
    let store_id = req.store_id.expect("must have");

    let mut conn = PgConnection::establish(&state.pg_url).unwrap();
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
) -> Result<(HeaderMap, Json<Vec<Value>>), (StatusCode, HeaderMap, String)> {
    let origin = validate_cors(origin)?;
    let mut headers = create_cors_headers(&origin);
    headers.insert(CONTENT_TYPE, "application/json".parse().unwrap());

    let store_id = verify_token(token.token(), &state, &headers)?;

    ensure_store_id!(payload, store_id, headers);

    match list_key_versions_impl(payload, &state).await {
        Ok(res) => Ok((headers, Json(res))),
        Err(e) => Err(handle_anyhow_error(e, headers)),
    }
}

pub async fn health_check() -> Result<Json<()>, (StatusCode, String)> {
    Ok(Json(()))
}

pub fn validate_cors(
    origin: Option<TypedHeader<Origin>>,
) -> Result<String, (StatusCode, HeaderMap, String)> {
    if let Some(TypedHeader(origin)) = origin {
        if origin.is_null() {
            return Ok("*".to_string());
        }

        let origin_str = origin.to_string();
        if ALLOWED_ORIGINS.contains(&origin_str.as_str())
            || origin_str.ends_with(ALLOWED_SUBDOMAIN)
            || origin_str.starts_with(ALLOWED_LOCALHOST)
        {
            return Ok(origin_str);
        } else {
            let headers = create_cors_headers("*");
            // The origin is not in the allowed list block the request
            return Err((StatusCode::NOT_FOUND, headers, String::new()));
        }
    }

    Ok("*".to_string())
}

pub fn create_cors_headers(origin: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(ACCESS_CONTROL_ALLOW_ORIGIN, origin.parse().unwrap());
    headers.insert(
        ACCESS_CONTROL_ALLOW_METHODS,
        "GET, POST, PUT, DELETE, OPTIONS".parse().unwrap(),
    );
    headers.insert(ACCESS_CONTROL_ALLOW_HEADERS, "*".parse().unwrap());
    headers
}

pub(crate) fn handle_anyhow_error(
    err: anyhow::Error,
    headers: HeaderMap,
) -> (StatusCode, HeaderMap, String) {
    error!("Error: {err:?}");
    (StatusCode::BAD_REQUEST, headers, format!("{err}"))
}
