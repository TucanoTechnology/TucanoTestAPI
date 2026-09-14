use tucano_test::{api, auth, repository};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let command = args.next();

    if command.as_deref() == Some("seed-auth") {
        return seed_auth(args.collect());
    }
    if command.is_some() {
        return Err(format!(
            "unknown command {:?}: the binary serves the API with no arguments, or seeds the \
             demo accounts with `seed-auth`",
            command.unwrap_or_default()
        )
        .into());
    }

    let data_dir = data_dir();
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

/// The data directory the server would use, so `seed-auth` writes where the
/// server reads.
fn data_dir() -> std::path::PathBuf {
    std::env::var_os("TUCANO_DATA_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("data"))
}

/// `seed-auth` — creates the demo account and grants of `docs/testing/seed-dataset-spec.md` §5.
///
/// The seed script drives everything else over HTTP, but the API publishes no
/// route that creates an account or records a grant, so this subcommand writes
/// them through the same [`auth::AuthStore`] the server uses. It is idempotent:
/// an account that already exists keeps its password and only the grants it is
/// missing are added.
///
/// ```text
/// tucano-test seed-auth --username viewer --password <password> \
///     --grant checkout.json=owner --grant payments.json=owner
/// ```
fn seed_auth(args: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    let mut username: Option<String> = None;
    let mut password: Option<String> = None;
    let mut grants: Vec<(String, String)> = Vec::new();

    let mut args = args.into_iter();
    while let Some(flag) = args.next() {
        let value = args.next().ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--username" => username = Some(value),
            "--password" => password = Some(value),
            "--grant" => {
                let (project, role) = value
                    .split_once('=')
                    .ok_or_else(|| format!("--grant expects <project>=<role>, got {value:?}"))?;
                if project.is_empty() {
                    return Err(format!("--grant names no project: {value:?}").into());
                }
                // Validate the role now, so a typo fails before anything is written.
                auth::parse_role(role)?;
                grants.push((project.to_owned(), role.to_owned()));
            }
            other => {
                return Err(format!(
                    "unknown option {other:?}; expected --username, --password or --grant"
                )
                .into());
            }
        }
    }

    let username = username.ok_or("seed-auth needs --username")?;
    let password = password.ok_or("seed-auth needs --password")?;

    let store = auth::AuthStore::new(data_dir())?;
    let seeded = auth::seed_account(
        &store,
        &auth::AccountSpec {
            username,
            password,
            system_admin: false,
            grants,
        },
    )?;

    let action = if seeded.created {
        "created"
    } else {
        "already present"
    };
    println!("seed-auth: account {} {}", seeded.username, action);
    for (project, role) in &seeded.grants_added {
        println!("seed-auth: granted {role} on {project}");
    }
    Ok(())
}

/// Seconds since the Unix epoch: the instant the bootstrap account is stamped
/// with.
fn now_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since_epoch| since_epoch.as_secs())
}
