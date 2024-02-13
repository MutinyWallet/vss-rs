use crate::auth::verify_token;
use crate::kv::{KeyValue, KeyValueOld};
use crate::{State, ALLOWED_LOCALHOST, ALLOWED_ORIGINS, ALLOWED_SUBDOMAIN, API_VERSION};
use axum::headers::authorization::Bearer;
use axum::headers::{Authorization, Origin};
use axum::http::StatusCode;
use axum::{Extension, Json, TypedHeader};
use log::{debug, error, trace};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

macro_rules! ensure_store_id {
    ($payload:ident, $store_id:expr) => {
        match $payload.store_id {
            None => {
                // if neither has a store id, return an error
                if $store_id.is_none() {
                    return Err((
                        StatusCode::UNAUTHORIZED,
                        format!("Unauthorized: store_id required"),
                    ));
                }
                $payload.store_id = $store_id
            }
            Some(ref id) => match $store_id {
                None => (),
                Some(ref store_id) => {
                    // if both have a store id, make sure they match
                    if id != store_id {
                        return Err((
                            StatusCode::UNAUTHORIZED,
                            format!("Unauthorized: store_id mismatch"),
                        ));
                    }
                }
            },
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

    let backend = state.backend.clone();
    let item = backend.get_item(&store_id, &req.key)?;

    Ok(item.and_then(|i| i.into_kv()))
}

/// Returns value as base64-encoded string
pub async fn get_object(
    origin: Option<TypedHeader<Origin>>,
    auth: Option<TypedHeader<Authorization<Bearer>>>,
    Extension(state): Extension<State>,
    Json(mut payload): Json<GetObjectRequest>,
) -> Result<Json<Option<KeyValueOld>>, (StatusCode, String)> {
    debug!("get_object: {payload:?}");
    if !state.self_hosted {
        validate_cors(origin)?;
    }

    let store_id = auth
        .map(|TypedHeader(token)| verify_token(token.token(), &state))
        .transpose()?
        .flatten();

    ensure_store_id!(payload, store_id);

    match get_object_impl(payload, &state).await {
        Ok(Some(res)) => Ok(Json(Some(res.into()))),
        Ok(None) => Ok(Json(None)),
        Err(e) => Err(handle_anyhow_error("get_object", e)),
    }
}

/// Returns value as a byte array
pub async fn get_object_v2(
    origin: Option<TypedHeader<Origin>>,
    auth: Option<TypedHeader<Authorization<Bearer>>>,
    Extension(state): Extension<State>,
    Json(mut payload): Json<GetObjectRequest>,
) -> Result<Json<Option<KeyValue>>, (StatusCode, String)> {
    debug!("get_object v2: {payload:?}");
    if !state.self_hosted {
        validate_cors(origin)?;
    }

    let store_id = auth
        .map(|TypedHeader(token)| verify_token(token.token(), &state))
        .transpose()?
        .flatten();

    ensure_store_id!(payload, store_id);

    match get_object_impl(payload, &state).await {
        Ok(res) => Ok(Json(res)),
        Err(e) => Err(handle_anyhow_error("get_object_v2", e)),
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

    let backend = state.backend.clone();
    backend.put_items_in_store(&store_id, req.transaction_items)?;

    Ok(())
}

pub async fn put_objects(
    origin: Option<TypedHeader<Origin>>,
    auth: Option<TypedHeader<Authorization<Bearer>>>,
    Extension(state): Extension<State>,
    Json(mut payload): Json<PutObjectsRequest>,
) -> Result<Json<()>, (StatusCode, String)> {
    if !state.self_hosted {
        validate_cors(origin)?;
    }

    let store_id = auth
        .map(|TypedHeader(token)| verify_token(token.token(), &state))
        .transpose()?
        .flatten();

    ensure_store_id!(payload, store_id);

    match put_objects_impl(payload, &state).await {
        Ok(res) => Ok(Json(res)),
        Err(e) => Err(handle_anyhow_error("put_objects", e)),
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

    let backend = state.backend.clone();
    let versions = backend.list_key_versions(&store_id, req.key_prefix.as_deref())?;

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
    auth: Option<TypedHeader<Authorization<Bearer>>>,
    Extension(state): Extension<State>,
    Json(mut payload): Json<ListKeyVersionsRequest>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    if !state.self_hosted {
        validate_cors(origin)?;
    }

    let store_id = auth
        .map(|TypedHeader(token)| verify_token(token.token(), &state))
        .transpose()?
        .flatten();

    ensure_store_id!(payload, store_id);

    match list_key_versions_impl(payload, &state).await {
        Ok(res) => Ok(Json(res)),
        Err(e) => Err(handle_anyhow_error("list_key_versions", e)),
    }
}

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

impl HealthResponse {
    /// Fabricate a status: pass response without checking database connectivity
    pub fn new_ok() -> Self {
        Self {
            status: String::from("pass"),
            version: String::from(API_VERSION),
        }
    }
}

/// IETF draft RFC for HTTP API Health Checks:
/// https://datatracker.ietf.org/doc/html/draft-inadarei-api-health-check
pub async fn health_check() -> Result<Json<HealthResponse>, (StatusCode, String)> {
    Ok(Json(HealthResponse::new_ok()))
}

pub fn valid_origin(origin: &str) -> bool {
    ALLOWED_ORIGINS.contains(&origin)
        || origin.ends_with(ALLOWED_SUBDOMAIN)
        || origin.starts_with(ALLOWED_LOCALHOST)
}

pub fn validate_cors(origin: Option<TypedHeader<Origin>>) -> Result<(), (StatusCode, String)> {
    if let Some(TypedHeader(origin)) = origin {
        if origin.is_null() {
            return Ok(());
        }

        let origin_str = origin.to_string();
        if valid_origin(&origin_str) {
            return Ok(());
        } else {
            // The origin is not in the allowed list block the request
            return Err((StatusCode::NOT_FOUND, String::new()));
        }
    }

    Ok(())
}

pub(crate) fn handle_anyhow_error(function: &str, err: anyhow::Error) -> (StatusCode, String) {
    error!("Error in {function}: {err:?}");
    (StatusCode::BAD_REQUEST, format!("{err}"))
}
