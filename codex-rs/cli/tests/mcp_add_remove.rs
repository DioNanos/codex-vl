use std::path::Path;

use anyhow::Result;
use codex_config::types::McpServerTransportConfig;
use codex_core::config::load_global_mcp_servers;
use predicates::str::contains;
use pretty_assertions::assert_eq;
use serde_json::Value as JsonValue;
use tempfile::TempDir;

fn codex_command(codex_home: &Path) -> Result<assert_cmd::Command> {
    let mut cmd = assert_cmd::Command::new(codex_utils_cargo_bin::cargo_bin("codex")?);
    cmd.env("CODEX_HOME", codex_home);
    Ok(cmd)
}

#[tokio::test]
async fn add_and_remove_server_updates_global_config() -> Result<()> {
    let codex_home = TempDir::new()?;

    let mut add_cmd = codex_command(codex_home.path())?;
    add_cmd
        .args(["mcp", "add", "docs", "--", "echo", "hello"])
        .assert()
        .success()
        .stdout(contains("Added global MCP server 'docs'."));

    let servers = load_global_mcp_servers(codex_home.path()).await?;
    assert_eq!(servers.len(), 1);
    let docs = servers.get("docs").expect("server should exist");
    match &docs.transport {
        McpServerTransportConfig::Stdio {
            command,
            args,
            env,
            env_vars,
            cwd,
        } => {
            assert_eq!(command, "echo");
            assert_eq!(args, &vec!["hello".to_string()]);
            assert!(env.is_none());
            assert!(env_vars.is_empty());
            assert!(cwd.is_none());
        }
        other => panic!("unexpected transport: {other:?}"),
    }
    assert!(docs.enabled);

    let mut remove_cmd = codex_command(codex_home.path())?;
    remove_cmd
        .args(["mcp", "remove", "docs"])
        .assert()
        .success()
        .stdout(contains("Removed global MCP server 'docs'."));

    let servers = load_global_mcp_servers(codex_home.path()).await?;
    assert!(servers.is_empty());

    let mut remove_again_cmd = codex_command(codex_home.path())?;
    remove_again_cmd
        .args(["mcp", "remove", "docs"])
        .assert()
        .success()
        .stdout(contains("No MCP server named 'docs' found."));

    let servers = load_global_mcp_servers(codex_home.path()).await?;
    assert!(servers.is_empty());

    Ok(())
}

#[tokio::test]
async fn profile_mcp_reports_legacy_profile_migration() -> Result<()> {
    let codex_home = TempDir::new()?;
    std::fs::write(
        codex_home.path().join("config.toml"),
        r#"[profiles.work]
model = "gpt-5"
"#,
    )?;

    let mut list_cmd = codex_command(codex_home.path())?;
    list_cmd
        .args(["--profile", "work", "mcp", "list"])
        .assert()
        .failure()
        .stderr(contains("--profile `work` cannot be used"))
        .stderr(contains("[profiles.work]"))
        .stderr(contains("work.config.toml"));

    Ok(())
}

#[tokio::test]
async fn add_with_env_preserves_key_order_and_values() -> Result<()> {
    let codex_home = TempDir::new()?;

    let mut add_cmd = codex_command(codex_home.path())?;
    add_cmd
        .args([
            "mcp",
            "add",
            "envy",
            "--env",
            "FOO=bar",
            "--env",
            "ALPHA=beta",
            "--",
            "python",
            "server.py",
        ])
        .assert()
        .success();

    let servers = load_global_mcp_servers(codex_home.path()).await?;
    let envy = servers.get("envy").expect("server should exist");
    let env = match &envy.transport {
        McpServerTransportConfig::Stdio { env: Some(env), .. } => env,
        other => panic!("unexpected transport: {other:?}"),
    };

    assert_eq!(env.len(), 2);
    assert_eq!(env.get("FOO"), Some(&"bar".to_string()));
    assert_eq!(env.get("ALPHA"), Some(&"beta".to_string()));
    assert!(envy.enabled);

    Ok(())
}

#[tokio::test]
async fn add_streamable_http_without_manual_token() -> Result<()> {
    let codex_home = TempDir::new()?;

    let mut add_cmd = codex_command(codex_home.path())?;
    add_cmd
        .args(["mcp", "add", "github", "--url", "https://example.com/mcp"])
        .assert()
        .success();

    let servers = load_global_mcp_servers(codex_home.path()).await?;
    let github = servers.get("github").expect("github server should exist");
    match &github.transport {
        McpServerTransportConfig::StreamableHttp {
            url,
            bearer_token_env_var,
            http_headers,
            env_http_headers,
        } => {
            assert_eq!(url, "https://example.com/mcp");
            assert!(bearer_token_env_var.is_none());
            assert!(http_headers.is_none());
            assert!(env_http_headers.is_none());
        }
        other => panic!("unexpected transport: {other:?}"),
    }
    assert!(github.enabled);

    assert!(!codex_home.path().join(".credentials.json").exists());
    assert!(!codex_home.path().join(".env").exists());

    Ok(())
}

#[tokio::test]
async fn add_streamable_http_with_custom_env_var() -> Result<()> {
    let codex_home = TempDir::new()?;

    let mut add_cmd = codex_command(codex_home.path())?;
    add_cmd
        .args([
            "mcp",
            "add",
            "issues",
            "--url",
            "https://example.com/issues",
            "--bearer-token-env-var",
            "GITHUB_TOKEN",
        ])
        .assert()
        .success();

    let servers = load_global_mcp_servers(codex_home.path()).await?;
    let issues = servers.get("issues").expect("issues server should exist");
    match &issues.transport {
        McpServerTransportConfig::StreamableHttp {
            url,
            bearer_token_env_var,
            http_headers,
            env_http_headers,
        } => {
            assert_eq!(url, "https://example.com/issues");
            assert_eq!(bearer_token_env_var.as_deref(), Some("GITHUB_TOKEN"));
            assert!(http_headers.is_none());
            assert!(env_http_headers.is_none());
        }
        other => panic!("unexpected transport: {other:?}"),
    }
    assert!(issues.enabled);
    Ok(())
}

#[tokio::test]
async fn add_streamable_http_with_oauth_options() -> Result<()> {
    let codex_home = TempDir::new()?;

    let mut add_cmd = codex_command(codex_home.path())?;
    add_cmd
        .args([
            "mcp",
            "add",
            "oauth-server",
            "--url",
            "https://example.com/mcp",
            "--oauth-client-id",
            "eci-prd-pub-codex-123",
            "--oauth-resource",
            "https://resource.example.com",
        ])
        .assert()
        .success();

    let servers = load_global_mcp_servers(codex_home.path()).await?;
    let oauth_server = servers
        .get("oauth-server")
        .expect("oauth server should exist");
    assert_eq!(
        oauth_server.oauth_client_id(),
        Some("eci-prd-pub-codex-123")
    );
    assert_eq!(
        oauth_server.oauth_resource.as_deref(),
        Some("https://resource.example.com")
    );

    Ok(())
}

#[tokio::test]
async fn add_streamable_http_rejects_removed_flag() -> Result<()> {
    let codex_home = TempDir::new()?;

    let mut add_cmd = codex_command(codex_home.path())?;
    add_cmd
        .args([
            "mcp",
            "add",
            "github",
            "--url",
            "https://example.com/mcp",
            "--with-bearer-token",
        ])
        .assert()
        .failure()
        .stderr(contains("--with-bearer-token"));

    let servers = load_global_mcp_servers(codex_home.path()).await?;
    assert!(servers.is_empty());

    Ok(())
}

#[tokio::test]
async fn add_cant_add_command_and_url() -> Result<()> {
    let codex_home = TempDir::new()?;

    let mut add_cmd = codex_command(codex_home.path())?;
    add_cmd
        .args([
            "mcp",
            "add",
            "github",
            "--url",
            "https://example.com/mcp",
            "--command",
            "--",
            "echo",
            "hello",
        ])
        .assert()
        .failure()
        .stderr(contains("unexpected argument '--command' found"));

    let servers = load_global_mcp_servers(codex_home.path()).await?;
    assert!(servers.is_empty());

    Ok(())
}

#[tokio::test]
async fn add_with_env_var_persists_name_only() -> Result<()> {
    let codex_home = TempDir::new()?;

    // Even with a live value present in the child environment, the CLI must
    // persist only the name and never copy the value into the config file.
    let mut add_cmd = codex_command(codex_home.path())?;
    add_cmd
        .env("NEXUSCREW_MCP_SESSION", "super-secret-value-do-not-persist")
        .args([
            "mcp",
            "add",
            "nexuscrew",
            "--env-var",
            "NEXUSCREW_MCP_SESSION",
            "--",
            "nexuscrew",
            "mcp",
        ])
        .assert()
        .success()
        .stdout(contains("Added global MCP server 'nexuscrew'."));

    let servers = load_global_mcp_servers(codex_home.path()).await?;
    let nexuscrew = servers.get("nexuscrew").expect("server should exist");
    match &nexuscrew.transport {
        McpServerTransportConfig::Stdio {
            env, env_vars, cwd, ..
        } => {
            assert_eq!(env_vars, &vec!["NEXUSCREW_MCP_SESSION".into()]);
            assert!(env.is_none(), "no literal env map should be written");
            assert!(cwd.is_none());
        }
        other => panic!("unexpected transport: {other:?}"),
    }

    // The value must never reach the config file: only the name is stored.
    let config = std::fs::read_to_string(codex_home.path().join("config.toml"))?;
    assert!(config.contains("NEXUSCREW_MCP_SESSION"));
    assert!(
        !config.contains("super-secret-value-do-not-persist"),
        "env value leaked into config file"
    );

    Ok(())
}

#[tokio::test]
async fn add_with_multiple_env_vars_preserves_order() -> Result<()> {
    let codex_home = TempDir::new()?;

    codex_command(codex_home.path())?
        .args([
            "mcp",
            "add",
            "srv",
            "--env-var",
            "FOO",
            "--env-var",
            "BAR",
            "--env-var",
            "BAZ",
            "--",
            "echo",
        ])
        .assert()
        .success();

    let servers = load_global_mcp_servers(codex_home.path()).await?;
    match &servers.get("srv").expect("server should exist").transport {
        McpServerTransportConfig::Stdio { env_vars, .. } => {
            assert_eq!(env_vars, &vec!["FOO".into(), "BAR".into(), "BAZ".into()]);
        }
        other => panic!("unexpected transport: {other:?}"),
    }

    Ok(())
}

#[tokio::test]
async fn add_with_env_var_dedups_preserving_first_occurrence() -> Result<()> {
    let codex_home = TempDir::new()?;

    codex_command(codex_home.path())?
        .args([
            "mcp",
            "add",
            "srv",
            "--env-var",
            "FOO",
            "--env-var",
            "BAR",
            "--env-var",
            "FOO",
            "--env-var",
            "BAZ",
            "--env-var",
            "BAR",
            "--",
            "echo",
        ])
        .assert()
        .success();

    let servers = load_global_mcp_servers(codex_home.path()).await?;
    match &servers.get("srv").expect("server should exist").transport {
        McpServerTransportConfig::Stdio { env_vars, .. } => {
            assert_eq!(env_vars, &vec!["FOO".into(), "BAR".into(), "BAZ".into()]);
        }
        other => panic!("unexpected transport: {other:?}"),
    }

    Ok(())
}

#[tokio::test]
async fn add_with_env_var_and_env_coexist() -> Result<()> {
    let codex_home = TempDir::new()?;

    codex_command(codex_home.path())?
        .args([
            "mcp",
            "add",
            "srv",
            "--env",
            "FOO=bar",
            "--env-var",
            "BAZ",
            "--",
            "echo",
        ])
        .assert()
        .success();

    let servers = load_global_mcp_servers(codex_home.path()).await?;
    match &servers.get("srv").expect("server should exist").transport {
        McpServerTransportConfig::Stdio { env, env_vars, .. } => {
            let env = env.as_ref().expect("literal env map should be present");
            assert_eq!(env.get("FOO"), Some(&"bar".to_string()));
            assert_eq!(env_vars, &vec!["BAZ".into()]);
        }
        other => panic!("unexpected transport: {other:?}"),
    }

    Ok(())
}

#[tokio::test]
async fn add_with_invalid_env_var_name_fails_without_writing() -> Result<()> {
    // Each invalid name must be rejected by the value parser before any config
    // write. Use a fresh CODEX_HOME per case so a buggy write in one case
    // cannot mask another.
    for invalid in ["FOO=bar", "1FOO", "FOO-BAR", "FOO BAR", ""] {
        let codex_home = TempDir::new()?;
        codex_command(codex_home.path())?
            .args(["mcp", "add", "srv", "--env-var", invalid, "--", "echo"])
            .assert()
            .failure();
        let servers = load_global_mcp_servers(codex_home.path()).await?;
        assert!(
            servers.is_empty(),
            "config was mutated for invalid env-var name `{invalid}`"
        );
    }

    Ok(())
}

#[tokio::test]
async fn add_streamable_http_rejects_env_var() -> Result<()> {
    let codex_home = TempDir::new()?;

    codex_command(codex_home.path())?
        .args([
            "mcp",
            "add",
            "github",
            "--url",
            "https://example.com/mcp",
            "--env-var",
            "FOO",
        ])
        .assert()
        .failure()
        .stderr(contains("--env-var"));

    let servers = load_global_mcp_servers(codex_home.path()).await?;
    assert!(servers.is_empty());

    Ok(())
}

#[test]
fn add_env_var_appears_in_help() -> Result<()> {
    let codex_home = TempDir::new()?;

    let output = codex_command(codex_home.path())?
        .args(["mcp", "add", "--help"])
        .output()?;
    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout)?;
    assert!(
        stdout.contains("--env-var"),
        "help should document --env-var"
    );
    assert!(
        stdout.contains("<ENV_VAR>"),
        "help should show the ENV_VAR value placeholder"
    );

    Ok(())
}

#[tokio::test]
async fn add_with_env_var_round_trips_through_list_and_get_json() -> Result<()> {
    let codex_home = TempDir::new()?;

    codex_command(codex_home.path())?
        .args([
            "mcp",
            "add",
            "docs",
            "--env-var",
            "APP_TOKEN",
            "--env-var",
            "WORKSPACE_ID",
            "--",
            "docs-server",
            "--port",
            "4000",
        ])
        .assert()
        .success();

    // TOML round-trip: only the names are persisted, in insertion order.
    let servers = load_global_mcp_servers(codex_home.path()).await?;
    match &servers.get("docs").expect("server should exist").transport {
        McpServerTransportConfig::Stdio { env_vars, .. } => {
            assert_eq!(env_vars, &vec!["APP_TOKEN".into(), "WORKSPACE_ID".into()]);
        }
        other => panic!("unexpected transport: {other:?}"),
    }

    // `mcp list --json` must surface the allowlisted names and no values.
    let list_json = codex_command(codex_home.path())?
        .args(["mcp", "list", "--json"])
        .output()?;
    assert!(list_json.status.success());
    let parsed: JsonValue = serde_json::from_str(&String::from_utf8(list_json.stdout)?)?;
    assert_eq!(
        &parsed[0]["transport"]["env_vars"],
        &serde_json::json!(["APP_TOKEN", "WORKSPACE_ID"]),
        "list --json must show env_var names only"
    );

    // `mcp get` must render the names with masked values.
    let get_output = codex_command(codex_home.path())?
        .args(["mcp", "get", "docs"])
        .output()?;
    assert!(get_output.status.success());
    let stdout = String::from_utf8(get_output.stdout)?;
    assert!(stdout.contains("APP_TOKEN=*****"));
    assert!(stdout.contains("WORKSPACE_ID=*****"));

    Ok(())
}
