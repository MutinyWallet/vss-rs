use crate::models::MIGRATIONS;
use crate::routes::*;
use axum::http::{Method, StatusCode, Uri};
use axum::routing::{get, post, put};
use axum::{http, Extension, Router};
use diesel::r2d2::{ConnectionManager, Pool};
use diesel::PgConnection;
use diesel_migrations::MigrationHarness;
use secp256k1::{All, PublicKey, Secp256k1};
use tower_http::cors::{Any, CorsLayer};

mod auth;
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
    auth_key: PublicKey,
    secp: Secp256k1<All>,
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

    // DB management
    let manager = ConnectionManager::<PgConnection>::new(&pg_url);
    let db_pool = Pool::builder()
        .max_size(16) // TODO should this be bigger?
        .test_on_check_out(true)
        .build(manager)
        .expect("Could not build connection pool");

    // run migrations
    let mut connection = db_pool.get()?;
    // TODO not sure if code should handle the migration, we probably need to shut down
    // then migrate and then boot back up since there could be multiple instances running
    connection
        .run_pending_migrations(MIGRATIONS)
        .expect("migrations could not run");

    let secp = Secp256k1::new();

    let state = State {
        db_pool,
        auth_key,
        secp,
    };

    let addr: std::net::SocketAddr = format!("0.0.0.0:{port}")
        .parse()
        .expect("Failed to parse bind/port for webserver");

    let server_router = Router::new()
        .route("/health-check", get(health_check))
        .route("/getObject", post(get_object))
        .route("/putObjects", put(put_objects))
        .route("/listKeyVersions", post(list_key_versions))
        .route("/migration", get(migration::migration))
        .fallback(fallback)
        .layer(Extension(state.clone()))
        .layer(
            CorsLayer::new()
                .allow_origin(Any) // TODO do not allow all
                .allow_headers(vec![
                    http::header::CONTENT_TYPE,
                    http::header::AUTHORIZATION,
                ])
                .allow_methods([Method::GET, Method::POST, Method::PUT, Method::OPTIONS]), // delete?
        );

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

async fn fallback(uri: Uri) -> (StatusCode, String) {
    (StatusCode::NOT_FOUND, format!("No route for {uri}"))
}
