mod api;
mod auth;
mod config;
mod danmaku;
mod error;
mod http_client;
mod identity;
mod models;
mod pb;
mod sites;
mod sign;
mod state;
mod store;
mod tars;
mod util;

use clap::Parser;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing_subscriber::EnvFilter;

use config::{Cli, Config};
use state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let cli = Cli::parse();
    let cfg = Config::load(&cli)?;
    let listen = cfg.server.listen.clone();

    let st = AppState::build(cfg).await?;
    tracing::info!("鉴权：{}", if st.auth.enabled() { "已开启" } else { "关闭" });
    let app = api::router(st)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(&listen).await?;
    tracing::info!("简播服务已启动：http://{listen}");
    axum::serve(listener, app).await?;
    Ok(())
}
