use crate::config;
use anyhow::Result;

pub async fn doctor() -> Result<()> {
    let cfg = config::load()?;
    println!("ClearMesh doctor");
    println!("API: {}", cfg.api);
    println!("Authenticated: {}", cfg.token.is_some());
    match reqwest::get(format!("{}/health", cfg.api.trim_end_matches('/'))).await {
        Ok(response) => println!("API health: {}", response.status()),
        Err(err) => println!("API health: unavailable ({err})"),
    }
    println!("Mount: read-only Linux/FUSE3 supported");
    println!("Vault fetch order: encrypted cache -> Vault");
    println!("Vault backend: configured on API");
    Ok(())
}
