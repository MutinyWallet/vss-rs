use crate::models::MIGRATIONS;
use crate::routes::*;
use axum::extract::DefaultBodyLimit;
use axum::headers::Origin;
use axum::http::{request::Parts, HeaderValue, Method, StatusCode, Uri};
use axum::routing::{get, post, put};
use axum::{http, Extension, Router, TypedHeader};
use diesel::r2d2::{ConnectionManager, Pool};
use diesel::PgConnection;
use diesel_migrations::MigrationHarness;
use secp256k1::{All, PublicKey, Secp256k1};
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
    db_pool: Pool<ConnectionManager<PgConnection>>,
    pub auth_key: Option<PublicKey>,
    pub self_hosted: bool,
    pub secp: Secp256k1<All>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env file
    dotenv::dotenv().ok();
    pretty_env_logger::try_init()?;

    // get values key from env
    let pg_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let port: u16 = std::env::var("VSS_PORT")
        .ok()
        .map(|p| p.parse::<u16>())
        .transpose()?
        .unwrap_or(8080);

    let auth_key = std::env::var("AUTH_KEY").ok();
    let auth_key = match auth_key {
        None => None,
        Some(data) => {
            let auth_key_bytes = hex::decode(data)?;
            Some(PublicKey::from_slice(&auth_key_bytes)?)
        }
    };

    // DB management
    let manager = ConnectionManager::<PgConnection>::new(&pg_url);
    let db_pool = Pool::builder()
        .max_size(10) // should be a multiple of 100, our database connection limit
        .test_on_check_out(true)
        .build(manager)
        .expect("Could not build connection pool");

    let secp = Secp256k1::new();

    let self_hosted = std::env::var("SELF_HOST")
        .ok()
        .map(|s| s == "true" || s == "1")
        .unwrap_or(false);

    // run migrations if self hosted, otherwise assume they have been run manually
    if self_hosted {
        let mut connection = db_pool.get()?;
        connection
            .run_pending_migrations(MIGRATIONS)
            .expect("migrations could not run");
    }

    let state = State {
        db_pool,
        auth_key,
        self_hosted,
        secp,
    };

    let addr: std::net::SocketAddr = format!("0.0.0.0:{port}")
        .parse()
        .expect("Failed to parse bind/port for webserver");

    // if the server is self hosted, allow all origins
    // otherwise, only allow the origins in ALLOWED_ORIGINS
    let cors_function = if self_hosted {
        |_: &HeaderValue, _request_parts: &Parts| true
    } else {
        |origin: &HeaderValue, _request_parts: &Parts| {
            let Ok(origin) = origin.to_str() else {
                return false;
            };

            valid_origin(origin)
        }
    };

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
                .allow_origin(AllowOrigin::predicate(cors_function))
                .allow_headers([http::header::CONTENT_TYPE, http::header::AUTHORIZATION])
                .allow_methods([
                    Method::GET,
                    Method::POST,
                    Method::PUT,
                    Method::DELETE,
                    Method::OPTIONS,
                ]),
        )
        .layer(DefaultBodyLimit::max(100_000_000)) // max 100mb body size
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
