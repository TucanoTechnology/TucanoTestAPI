use std::time::Duration;

use tucano_test::{api, auth, config, repository};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let command = args.next();

    if command.as_deref() == Some("seed-auth") {
        return seed_auth(args.collect());
    }
    if command.as_deref() == Some("unseed-auth") {
        return unseed_auth(args.collect());
    }
    if command.is_some() {
        return Err(format!(
            "unknown command {:?}: the binary serves the API with no arguments, seeds the demo \
             accounts with `seed-auth`, or removes them again with `unseed-auth`",
            command.unwrap_or_default()
        )
        .into());
    }

    // The optional file is read once, here, before the listener binds: the ADR
    // puts it in the same class as the environment, resolved at startup into an
    // immutable value, so a bad one is a startup failure rather than a request
    // failure. No `TUCANO_CONFIG_FILE` means no file, which is the pre-file
    // behaviour of every deployment that predates this module.
    let file = config::load_from_env()?;

    let data_dir = data_dir();
    let port = port();
    let lock_timeout = lock_timeout();
    let repository =
        repository::FileRepository::new(data_dir.clone())?.with_lock_timeout(lock_timeout);

    let auth_config = auth::AuthConfig::from_env_and_file(file.as_ref())?;
    let store = auth::AuthStore::new(&data_dir)?.with_lock_timeout(lock_timeout);
    auth::ensure_bootstrap_user(&store, &auth_config, now_seconds())?;
    let authentication = api::auth::AuthState::new(store, auth_config);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    axum::serve(listener, api::router(repository, authentication)).await?;
    Ok(())
}

/// The data directory the server would use, so `seed-auth` writes where the
/// server reads.
///
/// `TUCANO_DATA_DIR` stays environment-only: it decides where the data volume
/// is, and the orchestrator must be able to set it before anything else —
/// including the configuration file — can be located.
fn data_dir() -> std::path::PathBuf {
    std::env::var_os("TUCANO_DATA_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("data"))
}

/// The port the listener binds, environment-only for the same reason as
/// [`data_dir`]: the orchestrator owns it, and it must be known before a file
/// could be read.
fn port() -> String {
    std::env::var("PORT").unwrap_or_else(|_| "3000".to_owned())
}

/// The millisecond count a write waits for the advisory lock by default, so an
/// unset `TUCANO_LOCK_TIMEOUT_MS` and one declared with no value agree.
const DEFAULT_LOCK_TIMEOUT_MS: u64 = 5000;

/// How long the advisory lock may be waited on before a write is refused with
/// a 503. Environment-only like [`data_dir`] and [`port`]: the orchestrator
/// owns it, and it must be known before anything touches the data directory.
///
/// Defaults to 5 000 ms when `TUCANO_LOCK_TIMEOUT_MS` is unset or empty; a
/// non-numeric value is a startup error.
fn lock_timeout() -> Duration {
    lock_timeout_from(std::env::var("TUCANO_LOCK_TIMEOUT_MS").ok().as_deref())
}

/// Reads the timeout from the raw environment value, so the parse is testable
/// without touching the process environment.
///
/// An unset, empty or blank value keeps the default: a variable declared with
/// no value — an `.env` line or a manifest key left for the operator to fill —
/// then behaves exactly like an absent one instead of stopping startup with a
/// message that shows the empty value as if a bad one had been passed (Issue
/// #309). Anything else must parse as a `u64`.
fn lock_timeout_from(raw: Option<&str>) -> Duration {
    let Some(raw) = raw.map(str::trim).filter(|raw| !raw.is_empty()) else {
        return Duration::from_millis(DEFAULT_LOCK_TIMEOUT_MS);
    };
    let millis: u64 = raw
        .parse()
        .unwrap_or_else(|_| panic!("TUCANO_LOCK_TIMEOUT_MS is not a valid u64: {raw:?}"));
    Duration::from_millis(millis)
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
///     --grant checkout.json=owner
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

/// `unseed-auth` — the inverse of `seed-auth`, and the auth half of the
/// teardown of `docs/testing/seed-dataset-spec.md` §4.
///
/// It forgets one named account and the grants that account holds on the named
/// projects, and nothing else: removal is by identifier and by project, never
/// by pattern. The bootstrap account is refused outright, and an account or
/// grant it cannot find is reported as left in place rather than guessed at —
/// "a missed deletion is recoverable; a deleted project is not".
///
/// It reports what it removed and what it kept, and exits non-zero only when
/// something it was asked to remove could not be resolved to an account it is
/// allowed to touch.
///
/// `--check` is the read-only half: it reports where the account and the named
/// grants stand and writes nothing, so a caller that was told "no such account"
/// can ask whether the name *it* was pointed at is merely not the one the seed
/// wrote. It is a mode of this subcommand rather than another one so the
/// documented pair of auth subcommands stays a pair.
///
/// ```text
/// tucano-test unseed-auth --username viewer \
///     --grant checkout.json
///
/// tucano-test unseed-auth --username viewer \
///     --grant checkout.json --check
/// ```
fn unseed_auth(args: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    let mut username: Option<String> = None;
    let mut grants: Vec<String> = Vec::new();
    let mut remove_account = true;
    let mut check = false;

    let mut args = args.into_iter();
    while let Some(flag) = args.next() {
        if flag == "--keep-account" {
            remove_account = false;
            continue;
        }
        if flag == "--check" {
            check = true;
            continue;
        }
        let value = args.next().ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--username" => username = Some(value),
            "--grant" => {
                if value.is_empty() {
                    return Err("--grant names no project".into());
                }
                grants.push(value);
            }
            other => {
                return Err(format!(
                    "unknown option {other:?}; expected --username, --grant, --keep-account or \
                     --check"
                )
                .into());
            }
        }
    }

    let username = username.ok_or("unseed-auth needs --username")?;
    if grants.is_empty() {
        return Err(
            "unseed-auth needs at least one --grant <project>: it removes only the grants it is \
             told about, never every grant an account holds"
                .into(),
        );
    }
    if check && !remove_account {
        return Err(
            "--check reads the account and writes nothing, so --keep-account changes nothing; \
             ask for one or the other"
                .into(),
        );
    }

    let store = auth::AuthStore::new(data_dir())?;

    if check {
        let report = auth::report_account(&store, &username, &grants)?;
        if report.present {
            println!(
                "unseed-auth: check account {} is present{}",
                report.username,
                if report.system_admin {
                    " and administers the server"
                } else {
                    ""
                }
            );
            for (project, role) in &report.grants_present {
                println!("unseed-auth: check holds the {role} grant on {project}");
            }
            for project in &report.grants_absent {
                println!("unseed-auth: check holds no grant on {project}");
            }
        } else {
            println!("unseed-auth: check account {username} is absent");
        }
        for (project, user_id, role) in &report.orphans {
            println!(
                "unseed-auth: check found a {role} grant on {project} for account {user_id}, \
                 which no account answers for"
            );
        }
        // A stable summary line for `scripts/teardown.mjs`, distinct from the
        // removal summary so a probe can never be read as a removal.
        println!(
            "unseed-auth: check account={} system_admin={} grants_present={} grants_absent={} \
             orphans={}",
            if report.present { "present" } else { "absent" },
            report.system_admin,
            report.grants_present.len(),
            report.grants_absent.len(),
            report.orphans.len(),
        );
        return Ok(());
    }

    let removed = auth::unseed_account(
        &store,
        &auth::UnseedSpec {
            username,
            grants,
            remove_account,
        },
    )?;

    if removed.account_removed {
        println!("unseed-auth: removed account {}", removed.username);
    } else if let Some(reason) = &removed.account_kept {
        println!("unseed-auth: kept account {} ({reason})", removed.username);
    }
    for (project, role) in &removed.grants_removed {
        println!("unseed-auth: removed the {role} grant on {project}");
    }
    for (project, reason) in &removed.grants_kept {
        println!("unseed-auth: kept the grant on {project} ({reason})");
    }
    for project in &removed.grants_absent {
        println!("unseed-auth: no grant on {project} to remove");
    }
    // A stable summary line for `scripts/teardown.mjs`, which drives this
    // subcommand and has to tell "already gone, nothing to do" from "there is
    // something here I am not allowed to touch". Matching the prose above would
    // conflate the two; this says which it is outright.
    println!(
        "unseed-auth: account={} grants_removed={} grants_kept={}",
        if removed.account_removed {
            "removed"
        } else if removed
            .account_kept
            .as_deref()
            .is_some_and(auth::is_missing_account_reason)
        {
            "absent"
        } else {
            "kept"
        },
        removed.grants_removed.len(),
        removed.grants_kept.len(),
    );
    Ok(())
}

/// Seconds since the Unix epoch: the instant the bootstrap account is stamped
/// with.
fn now_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since_epoch| since_epoch.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lock_timeout_is_read_from_the_value() {
        assert_eq!(
            lock_timeout_from(Some("15000")),
            Duration::from_millis(15000)
        );
        assert_eq!(
            lock_timeout_from(Some(" 250 ")),
            Duration::from_millis(250),
            "surrounding whitespace is not part of the number"
        );
    }

    #[test]
    fn an_unset_or_empty_lock_timeout_keeps_the_default() {
        for raw in [None, Some(""), Some("   ")] {
            assert_eq!(
                lock_timeout_from(raw),
                Duration::from_millis(DEFAULT_LOCK_TIMEOUT_MS),
                "{raw:?} should keep the default"
            );
        }
    }

    #[test]
    #[should_panic(expected = "TUCANO_LOCK_TIMEOUT_MS is not a valid u64")]
    fn a_non_numeric_lock_timeout_stops_startup() {
        lock_timeout_from(Some("soon"));
    }
}
