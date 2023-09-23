use crate::routes::*;
use axum::headers::Origin;
use axum::http::{request::Parts, HeaderValue, Method, StatusCode, Uri};
use axum::routing::{get, post, put};
use axum::{http, Extension, Router, TypedHeader};
use native_tls::TlsConnector;
use postgres_native_tls::MakeTlsConnector;
use secp256k1::{All, PublicKey, Secp256k1};
use std::str::FromStr;
use std::sync::Arc;
use tokio_postgres::{Client, Config};
use tower_http::cors::{AllowOrigin, CorsLayer};

mod auth;
mod kv;
mod migration;
mod models;
mod routes;

const ALLOWED_ORIGINS: [&str; 6] = [
    "https://app.mutinywallet.com",
    "capacitor://localhost",
    "https://signet-app.mutinywallet.com",
    "http://localhost:3420",
    "http://localhost",
    "https://localhost",
];

const ALLOWED_SUBDOMAIN: &str = ".mutiny-web.pages.dev";
const ALLOWED_LOCALHOST: &str = "http://127.0.0.1:";

#[derive(Clone)]
pub struct State {
    pub client: Arc<Client>,
    pub auth_key: PublicKey,
    pub secp: Secp256k1<All>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    pretty_env_logger::try_init()?;
    // Load .env file
    dotenv::dotenv().ok();

    // get values key from env
    let pg_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let auth_key = std::env::var("AUTH_KEY").expect("AUTH_KEY must be set");
    let port: u16 = std::env::var("VSS_PORT")
        .ok()
        .map(|p| p.parse::<u16>())
        .transpose()?
        .unwrap_or(8080);

    let auth_key_bytes = hex::decode(auth_key)?;
    let auth_key = PublicKey::from_slice(&auth_key_bytes)?;

    let tls = TlsConnector::new()?;
    let connector = MakeTlsConnector::new(tls);

    // Connect to the database.
    let mut config = Config::from_str(&pg_url).unwrap();
    config.pgbouncer_mode(true);
    let (client, connection) = config.connect(connector).await?;

    // The connection object performs the actual communication with the database,
    // so spawn it off to run on its own.
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            panic!("db connection error: {e}");
        }
    });

    let secp = Secp256k1::new();

    let state = State {
        client: Arc::new(client),
        auth_key,
        secp,
    };

    let addr: std::net::SocketAddr = format!("0.0.0.0:{port}")
        .parse()
        .expect("Failed to parse bind/port for webserver");

    let server_router = Router::new()
        .route("/health-check", get(health_check))
        .route("/getObject", post(get_object))
        .route("/v2/getObject", post(get_object_v2))
        .route("/putObjects", put(put_objects))
        .route("/v2/putObjects", put(put_objects))
        .route("/listKeyVersions", post(list_key_versions))
        .route("/v2/listKeyVersions", post(list_key_versions))
        .route("/migration", get(migration::migration))
        .fallback(fallback)
        .layer(
            CorsLayer::new()
                .allow_origin(AllowOrigin::predicate(
                    |origin: &HeaderValue, _request_parts: &Parts| {
                        let Ok(origin) = origin.to_str() else {
                            return false;
                        };

                        valid_origin(origin)
                    },
                ))
                .allow_headers([http::header::CONTENT_TYPE, http::header::AUTHORIZATION])
                .allow_methods([
                    Method::GET,
                    Method::POST,
                    Method::PUT,
                    Method::DELETE,
                    Method::OPTIONS,
                ]),
        )
        .layer(Extension(state));

    let server = axum::Server::bind(&addr).serve(server_router.into_make_service());

    println!("Webserver running on http://{addr}");

    let graceful = server.with_graceful_shutdown(async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to create Ctrl+C shutdown signal");
    });

    // Await the server to receive the shutdown signal
    if let Err(e) = graceful.await {
        eprintln!("shutdown error: {e}");
    }

    Ok(())
}

async fn fallback(origin: Option<TypedHeader<Origin>>, uri: Uri) -> (StatusCode, String) {
    if let Err((status, msg)) = validate_cors(origin) {
        return (status, msg);
    };

    (StatusCode::NOT_FOUND, format!("No route for {uri}"))
}
