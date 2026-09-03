use tucano_test::{api, repository};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = std::env::var_os("TUCANO_DATA_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("data"));
    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_owned());
    let repository = repository::FileRepository::new(data_dir)?;
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    axum::serve(listener, api::router(repository)).await?;
    Ok(())
}
