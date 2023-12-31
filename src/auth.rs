use crate::State;
use axum::http::StatusCode;
use jwt_compact::alg::Es256k;
use jwt_compact::{AlgorithmExt, TimeOptions, Token, UntrustedToken};
use log::error;
use secp256k1::PublicKey;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

pub(crate) fn verify_token(
    token: &str,
    state: &State,
) -> Result<Option<String>, (StatusCode, String)> {
    let Some(auth_key) = state.auth_key else {
        return Ok(None);
    };

    let es256k1 = Es256k::<Sha256>::new(state.secp.clone());

    validate_jwt_from_user(token, auth_key, &es256k1)
        .map(Some)
        .map_err(|e| {
            error!("Unauthorized: {e}");
            (StatusCode::UNAUTHORIZED, format!("Unauthorized: {e}"))
        })
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct CustomClaims {
    pub sub: String,
}

fn validate_jwt_from_user(
    token_str: &str,
    auth_key: PublicKey,
    es256k1: &Es256k<Sha256>,
) -> anyhow::Result<String> {
    let untrusted_token = UntrustedToken::new(token_str)?;

    let token: Token<CustomClaims> = es256k1.validator(&auth_key).validate(&untrusted_token)?;

    let time_options = TimeOptions::default();
    token.claims().validate_expiration(&time_options)?;
    token.claims().validate_maturity(&time_options)?;

    let claims = token.claims();

    Ok(claims.custom.sub.clone())
}
