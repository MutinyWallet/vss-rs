use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(version, author, about)]
/// VSS server connecting to a postgres database
pub struct Config {
    /// Postgres connection string
    #[clap(long)]
    pub pg_url: String,
    /// Bind address for zap-tunnel's webserver
    #[clap(default_value = "0.0.0.0", long)]
    pub bind: String,
    /// Port for zap-tunnel's webserver
    #[clap(default_value_t = 3000, long)]
    pub port: u16,
}
