//! Stdio entry point for the `SessionMesh` MCP server.

use std::error::Error;
use std::path::PathBuf;

use sessionmesh_mcp::{McpServer, resolve_current_global_id};
use sessionmesh_storage::Storage;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let home = std::env::var_os("SESSIONMESH_HOME")
        .map(PathBuf::from)
        .ok_or("SESSIONMESH_HOME is required")?;
    let storage = Storage::open(home.join("sessionmesh.db"), home.join("blobs")).await?;
    let project_root = std::env::var_os("SESSIONMESH_PROJECT_ROOT")
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok());
    let writes_authorized =
        std::env::var("SESSIONMESH_MCP_ALLOW_WRITES").is_ok_and(|value| value == "true");
    let current = resolve_current_global_id(&storage, project_root).await;
    let server = McpServer::new(storage, current, writes_authorized);
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut output = tokio::io::stdout();
    while let Some(line) = lines.next_line().await? {
        let request: serde_json::Value = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(_) => continue,
        };
        if let Some(response) = server.handle(request).await {
            output.write_all(response.to_string().as_bytes()).await?;
            output.write_all(b"\n").await?;
            output.flush().await?;
        }
    }
    Ok(())
}
