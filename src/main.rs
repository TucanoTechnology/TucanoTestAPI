use tucano_test::{api, auth, repository};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = std::env::var_os("TUCANO_DATA_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("data"));
    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_owned());
    let repository = repository::FileRepository::new(data_dir.clone())?;

    let config = auth::AuthConfig::from_env()?;
    let store = auth::AuthStore::new(&data_dir)?;
    auth::ensure_bootstrap_user(&store, &config, now_seconds())?;
    let authentication = api::auth::AuthState::new(store, config);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    axum::serve(listener, api::router(repository, authentication)).await?;
    Ok(())
}

/// Seconds since the Unix epoch: the instant the bootstrap account is stamped
/// with.
fn now_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since_epoch| since_epoch.as_secs())
}
