//! `SessionMesh` command-line entry point.
//!
//! Currently covers operator tooling for the network-ingestion collector
//! token lifecycle. Operates directly on the same local database the
//! daemon uses, resolved from the same configuration layers
//! (`sessionmesh_core::configuration`) — run it in the same host or
//! container as the daemon, e.g. via `docker exec`.

use std::collections::BTreeMap;
use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use sessionmesh_core::configuration::{ConfigInputs, ConfigLayer, resolve, user_config_path};
use sessionmesh_storage::{IngestionTokenRepository, Storage};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("collector") => run_collector(args).await,
        Some("--help" | "-h") | None => {
            print_usage();
            Ok(())
        }
        Some(other) => Err(format!("unknown command '{other}'\n\n{}", usage_text()).into()),
    }
}

async fn run_collector(mut args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    match args.next().as_deref() {
        Some("issue") => {
            let label = args
                .next()
                .ok_or("usage: sessionmesh collector issue <label>")?;
            issue(&open_storage().await?, &label).await
        }
        Some("list") => list(&open_storage().await?).await,
        Some("revoke") => {
            let id = args
                .next()
                .ok_or("usage: sessionmesh collector revoke <collector-id>")?;
            revoke(&open_storage().await?, &id).await
        }
        Some(other) => Err(format!(
            "unknown 'collector' subcommand '{other}'\n\n{}",
            usage_text()
        )
        .into()),
        None => Err(format!("missing 'collector' subcommand\n\n{}", usage_text()).into()),
    }
}

/// Opens the same database the daemon uses, resolved the same way
/// (`SESSIONMESH_HOME`, the user config file, then environment overrides).
async fn open_storage() -> Result<Storage, Box<dyn Error>> {
    let environment = env::vars().collect::<BTreeMap<_, _>>();
    let user_home = environment
        .get("HOME")
        .map(PathBuf::from)
        .ok_or("HOME is required")?;
    let user_config = fs::read_to_string(user_config_path(&user_home)).ok();
    let config = resolve(ConfigInputs {
        user_home: &user_home,
        platform_data_home: environment.get("XDG_DATA_HOME").map(Path::new),
        user_toml: user_config.as_deref(),
        environment: &environment,
        cli: ConfigLayer::default(),
    })?;
    Ok(Storage::open(&config.database_path.value, &config.blob_store_path.value).await?)
}

async fn issue(storage: &Storage, label: &str) -> Result<(), Box<dyn Error>> {
    let (collector, token) = storage.issue_collector(label, &now_rfc3339()).await?;
    println!("Collector issued: {}", collector.id);
    println!("Label:            {label}");
    println!();
    println!("Token (shown once, copy it now):");
    println!("  {token}");
    println!();
    println!(
        "Set this on the dedicated system as SESSIONMESH_COLLECTOR_TOKEN. \
         SessionMesh never stores or displays the raw token again; \
         if it is lost, revoke this collector and issue a new one."
    );
    Ok(())
}

async fn list(storage: &Storage) -> Result<(), Box<dyn Error>> {
    let collectors = storage.list_collectors().await?;
    if collectors.is_empty() {
        println!("No collectors registered.");
        return Ok(());
    }
    println!("{:<42} {:<24} {:<32} STATUS", "ID", "LABEL", "CREATED");
    for collector in collectors {
        let status = collector
            .revoked_at
            .as_deref()
            .map_or_else(|| "active".to_owned(), |at| format!("revoked {at}"));
        println!(
            "{:<42} {:<24} {:<32} {status}",
            collector.id, collector.label, collector.created_at
        );
    }
    Ok(())
}

async fn revoke(storage: &Storage, id: &str) -> Result<(), Box<dyn Error>> {
    if storage.revoke_collector(id, &now_rfc3339()).await? {
        println!("Collector {id} revoked; its token no longer authenticates.");
        Ok(())
    } else {
        Err(format!("no active collector with id '{id}' (already revoked or unknown)").into())
    }
}

fn now_rfc3339() -> String {
    chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now()).to_rfc3339()
}

fn usage_text() -> &'static str {
    "Usage:\n  \
     sessionmesh collector issue <label>     Issue a new collector token (shown once)\n  \
     sessionmesh collector list              List registered collectors\n  \
     sessionmesh collector revoke <id>       Revoke a collector's token"
}

fn print_usage() {
    println!("{}", usage_text());
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_storage() -> (tempfile::TempDir, Storage) {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let storage = Storage::open(
            directory.path().join("state.db"),
            directory.path().join("blobs"),
        )
        .await
        .expect("storage should open");
        (directory, storage)
    }

    #[tokio::test]
    async fn issue_list_and_revoke_round_trip() {
        let (_directory, storage) = test_storage().await;

        issue(&storage, "dedicated-host-1")
            .await
            .expect("issuing a collector should succeed");
        let collectors = storage.list_collectors().await.unwrap();
        assert_eq!(collectors.len(), 1);
        assert_eq!(collectors[0].label, "dedicated-host-1");
        assert!(collectors[0].revoked_at.is_none());

        list(&storage).await.expect("listing should succeed");

        revoke(&storage, &collectors[0].id)
            .await
            .expect("revoking an active collector should succeed");
        let collectors = storage.list_collectors().await.unwrap();
        assert!(collectors[0].revoked_at.is_some());
    }

    #[tokio::test]
    async fn revoking_an_unknown_collector_fails() {
        let (_directory, storage) = test_storage().await;
        let error = revoke(&storage, "collector_does_not_exist")
            .await
            .expect_err("revoking a non-existent collector must fail");
        assert!(error.to_string().contains("no active collector"));
    }

    #[tokio::test]
    async fn listing_with_no_collectors_does_not_fail() {
        let (_directory, storage) = test_storage().await;
        list(&storage).await.expect("empty listing should succeed");
    }
}
