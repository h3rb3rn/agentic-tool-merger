//! Typed configuration resolution with explicit provenance and safe path expansion.

use std::{
    collections::BTreeMap,
    error::Error,
    fmt::{self, Display, Formatter},
    net::{IpAddr, Ipv4Addr},
    path::{Path, PathBuf},
};

use serde::Deserialize;
use url::Url;

const DEFAULT_PORT: u16 = 8787;
const DEFAULT_DEBOUNCE_MS: u64 = 750;
const DEFAULT_TOKEN_BUDGET: u32 = 4_000;
const DEFAULT_CORRELATION_THRESHOLD: f64 = 0.8;

/// Identifies the layer that supplied an effective configuration value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigSource {
    /// Built-in, security-preserving defaults.
    Default,
    /// The user's TOML configuration file.
    User,
    /// A documented `SESSIONMESH_*` environment variable.
    Environment,
    /// An explicit command-line option.
    Cli,
}

/// Retains a resolved value together with the layer that supplied it.
#[derive(Clone, Debug, PartialEq)]
pub struct Resolved<T> {
    /// Effective value after all layers have been applied.
    pub value: T,
    /// Highest-precedence layer that supplied the effective value.
    pub source: ConfigSource,
}

impl<T> Resolved<T> {
    fn new(value: T, source: ConfigSource) -> Self {
        Self { value, source }
    }

    fn replace(&mut self, value: Option<T>, source: ConfigSource) {
        if let Some(value) = value {
            self.value = value;
            self.source = source;
        }
    }
}

impl<T> Resolved<Option<T>> {
    fn replace_present(&mut self, value: Option<T>, source: ConfigSource) {
        if let Some(value) = value {
            self.value = Some(value);
            self.source = source;
        }
    }
}

/// Fully resolved and validated `SessionMesh` configuration.
#[derive(Clone, Debug, PartialEq)]
pub struct AppConfig {
    /// Root directory for all SessionMesh-owned persistent state.
    pub home: Resolved<PathBuf>,
    /// Local API bind address.
    pub bind_address: Resolved<IpAddr>,
    /// Local API port.
    pub port: Resolved<u16>,
    /// Whether non-loopback binding or remote model endpoints are allowed.
    pub allow_network: Resolved<bool>,
    /// `SQLite` database location.
    pub database_path: Resolved<PathBuf>,
    /// Content-addressed blob-store location.
    pub blob_store_path: Resolved<PathBuf>,
    /// Filesystem watcher debounce duration in milliseconds.
    pub watch_debounce_ms: Resolved<u64>,
    /// Whether secret redaction is enabled before derived processing.
    pub redaction_enabled: Resolved<bool>,
    /// Optional OpenAI-compatible embedding endpoint.
    pub embedding_endpoint: Resolved<Option<Url>>,
    /// Optional OpenAI-compatible extraction and handoff endpoint.
    pub llm_endpoint: Resolved<Option<Url>>,
    /// Optional configured model identifier.
    pub model: Resolved<Option<String>>,
    /// Maximum tokens made available to a generated handoff.
    pub token_budget: Resolved<u32>,
    /// Minimum deterministic correlation score for automatic linking.
    pub correlation_threshold: Resolved<f64>,
    /// Explicit container-readable to host-origin path mappings.
    pub source_mappings: Resolved<Vec<SourceMapping>>,
}

/// Maps a readable runtime path to the original host path retained in provenance.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SourceMapping {
    /// Path visible to the `SessionMesh` process.
    pub readable_path: PathBuf,
    /// Original path label reported in event provenance.
    pub original_path: PathBuf,
}

/// Optional values supplied by a single configuration layer.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ConfigLayer {
    /// `SessionMesh` state home.
    pub home: Option<PathBuf>,
    /// API bind address.
    pub bind_address: Option<IpAddr>,
    /// API port.
    pub port: Option<u16>,
    /// Explicit permission for network access.
    pub allow_network: Option<bool>,
    /// `SQLite` database path.
    pub database_path: Option<PathBuf>,
    /// Blob-store path.
    pub blob_store_path: Option<PathBuf>,
    /// Watch debounce in milliseconds.
    pub watch_debounce_ms: Option<u64>,
    /// Secret-redaction toggle.
    pub redaction_enabled: Option<bool>,
    /// Embedding endpoint URL.
    pub embedding_endpoint: Option<String>,
    /// LLM endpoint URL.
    pub llm_endpoint: Option<String>,
    /// Model identifier.
    pub model: Option<String>,
    /// Handoff token budget.
    pub token_budget: Option<u32>,
    /// Automatic-correlation threshold.
    pub correlation_threshold: Option<f64>,
    /// Runtime-to-origin source mappings.
    pub source_mappings: Option<Vec<SourceMapping>>,
}

/// Inputs used to resolve configuration without reading ambient process state.
#[derive(Debug)]
pub struct ConfigInputs<'a> {
    /// Isolated user home used for defaults and `~` expansion.
    pub user_home: &'a Path,
    /// Optional platform data directory; defaults to `~/.local/share`.
    pub platform_data_home: Option<&'a Path>,
    /// Optional user TOML content.
    pub user_toml: Option<&'a str>,
    /// Explicit environment map.
    pub environment: &'a BTreeMap<String, String>,
    /// Explicit CLI layer.
    pub cli: ConfigLayer,
}

/// A field-specific configuration failure with its responsible source layer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigError {
    /// Dotted field or environment variable that failed validation.
    pub field: String,
    /// Layer responsible for the invalid value.
    pub source: ConfigSource,
    /// Human-readable explanation without secret-bearing input.
    pub message: String,
}

impl ConfigError {
    fn new(field: impl Into<String>, source: ConfigSource, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            source,
            message: message.into(),
        }
    }
}

impl Display for ConfigError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} from {:?}: {}",
            self.field, self.source, self.message
        )
    }
}

impl Error for ConfigError {}

/// Resolves all configuration layers in deterministic precedence order.
///
/// The function takes explicit inputs so callers and tests decide which
/// environment and home directory are visible. Invalid higher-precedence values
/// fail with their source instead of falling back to a lower layer.
///
/// # Errors
///
/// Returns [`ConfigError`] when a layer cannot be parsed, a path expression
/// references unavailable input, or the effective configuration violates a
/// security or range invariant.
pub fn resolve(inputs: ConfigInputs<'_>) -> Result<AppConfig, ConfigError> {
    if inputs.user_home.as_os_str().is_empty() {
        return Err(ConfigError::new(
            "user_home",
            ConfigSource::Default,
            "a non-empty user home is required",
        ));
    }

    let platform_data_home = inputs
        .platform_data_home
        .map_or_else(|| inputs.user_home.join(".local/share"), Path::to_path_buf);
    let default_home = platform_data_home.join("sessionmesh");
    let mut raw = RawConfig::defaults(default_home);

    if let Some(user_toml) = inputs.user_toml {
        let layer = toml::from_str::<ConfigLayer>(user_toml).map_err(|error| {
            ConfigError::new("user_config", ConfigSource::User, error.to_string())
        })?;
        raw.apply(layer, ConfigSource::User);
    }

    raw.apply(
        environment_layer(inputs.environment)?,
        ConfigSource::Environment,
    );
    raw.apply(inputs.cli, ConfigSource::Cli);
    raw.finish(inputs.user_home, inputs.environment)
}

/// Returns the platform-independent default user-configuration location.
#[must_use]
pub fn user_config_path(user_home: &Path) -> PathBuf {
    user_home.join(".config/sessionmesh/config.toml")
}

#[derive(Debug)]
struct RawConfig {
    home: Resolved<PathBuf>,
    bind_address: Resolved<IpAddr>,
    port: Resolved<u16>,
    allow_network: Resolved<bool>,
    database_path: Resolved<Option<PathBuf>>,
    blob_store_path: Resolved<Option<PathBuf>>,
    watch_debounce_ms: Resolved<u64>,
    redaction_enabled: Resolved<bool>,
    embedding_endpoint: Resolved<Option<String>>,
    llm_endpoint: Resolved<Option<String>>,
    model: Resolved<Option<String>>,
    token_budget: Resolved<u32>,
    correlation_threshold: Resolved<f64>,
    source_mappings: Resolved<Vec<SourceMapping>>,
}

impl RawConfig {
    fn defaults(home: PathBuf) -> Self {
        Self {
            home: Resolved::new(home, ConfigSource::Default),
            bind_address: Resolved::new(IpAddr::V4(Ipv4Addr::LOCALHOST), ConfigSource::Default),
            port: Resolved::new(DEFAULT_PORT, ConfigSource::Default),
            allow_network: Resolved::new(false, ConfigSource::Default),
            database_path: Resolved::new(None, ConfigSource::Default),
            blob_store_path: Resolved::new(None, ConfigSource::Default),
            watch_debounce_ms: Resolved::new(DEFAULT_DEBOUNCE_MS, ConfigSource::Default),
            redaction_enabled: Resolved::new(true, ConfigSource::Default),
            embedding_endpoint: Resolved::new(None, ConfigSource::Default),
            llm_endpoint: Resolved::new(None, ConfigSource::Default),
            model: Resolved::new(None, ConfigSource::Default),
            token_budget: Resolved::new(DEFAULT_TOKEN_BUDGET, ConfigSource::Default),
            correlation_threshold: Resolved::new(
                DEFAULT_CORRELATION_THRESHOLD,
                ConfigSource::Default,
            ),
            source_mappings: Resolved::new(Vec::new(), ConfigSource::Default),
        }
    }

    fn apply(&mut self, layer: ConfigLayer, source: ConfigSource) {
        self.home.replace(layer.home, source);
        self.bind_address.replace(layer.bind_address, source);
        self.port.replace(layer.port, source);
        self.allow_network.replace(layer.allow_network, source);
        self.database_path
            .replace_present(layer.database_path, source);
        self.blob_store_path
            .replace_present(layer.blob_store_path, source);
        self.watch_debounce_ms
            .replace(layer.watch_debounce_ms, source);
        self.redaction_enabled
            .replace(layer.redaction_enabled, source);
        self.embedding_endpoint
            .replace_present(layer.embedding_endpoint, source);
        self.llm_endpoint
            .replace_present(layer.llm_endpoint, source);
        self.model.replace_present(layer.model, source);
        self.token_budget.replace(layer.token_budget, source);
        self.correlation_threshold
            .replace(layer.correlation_threshold, source);
        self.source_mappings.replace(layer.source_mappings, source);
    }

    fn finish(
        self,
        user_home: &Path,
        environment: &BTreeMap<String, String>,
    ) -> Result<AppConfig, ConfigError> {
        let home = resolve_path(self.home, "home", user_home, environment)?;
        let database_path = resolve_optional_path(
            self.database_path,
            home.value.join("sessionmesh.db"),
            "database_path",
            user_home,
            environment,
        )?;
        let blob_store_path = resolve_optional_path(
            self.blob_store_path,
            home.value.join("blobs"),
            "blob_store_path",
            user_home,
            environment,
        )?;
        let source_mappings =
            resolve_source_mappings(self.source_mappings, user_home, environment)?;

        if !self.bind_address.value.is_loopback() && !self.allow_network.value {
            return Err(ConfigError::new(
                "bind_address",
                self.bind_address.source,
                "non-loopback binding requires allow_network = true",
            ));
        }
        if self.port.value == 0 {
            return Err(ConfigError::new(
                "port",
                self.port.source,
                "must be greater than zero",
            ));
        }
        if self.watch_debounce_ms.value > 60_000 {
            return Err(ConfigError::new(
                "watch_debounce_ms",
                self.watch_debounce_ms.source,
                "must not exceed 60000",
            ));
        }
        if self.token_budget.value == 0 {
            return Err(ConfigError::new(
                "token_budget",
                self.token_budget.source,
                "must be greater than zero",
            ));
        }
        if self.model.value.as_ref().is_some_and(String::is_empty) {
            return Err(ConfigError::new(
                "model",
                self.model.source,
                "must not be empty",
            ));
        }
        if !(0.0..=1.0).contains(&self.correlation_threshold.value) {
            return Err(ConfigError::new(
                "correlation_threshold",
                self.correlation_threshold.source,
                "must be between 0 and 1",
            ));
        }

        let embedding_endpoint = resolve_endpoint(
            self.embedding_endpoint,
            "embedding_endpoint",
            self.allow_network.value,
        )?;
        let llm_endpoint =
            resolve_endpoint(self.llm_endpoint, "llm_endpoint", self.allow_network.value)?;

        Ok(AppConfig {
            home,
            bind_address: self.bind_address,
            port: self.port,
            allow_network: self.allow_network,
            database_path,
            blob_store_path,
            watch_debounce_ms: self.watch_debounce_ms,
            redaction_enabled: self.redaction_enabled,
            embedding_endpoint,
            llm_endpoint,
            model: self.model,
            token_budget: self.token_budget,
            correlation_threshold: self.correlation_threshold,
            source_mappings,
        })
    }
}

fn environment_layer(environment: &BTreeMap<String, String>) -> Result<ConfigLayer, ConfigError> {
    Ok(ConfigLayer {
        home: environment.get("SESSIONMESH_HOME").map(PathBuf::from),
        bind_address: parse_environment(environment, "SESSIONMESH_BIND_ADDRESS")?,
        port: parse_environment(environment, "SESSIONMESH_PORT")?,
        allow_network: parse_environment(environment, "SESSIONMESH_ALLOW_NETWORK")?,
        database_path: environment
            .get("SESSIONMESH_DATABASE_PATH")
            .map(PathBuf::from),
        blob_store_path: environment
            .get("SESSIONMESH_BLOB_STORE_PATH")
            .map(PathBuf::from),
        watch_debounce_ms: parse_environment(environment, "SESSIONMESH_WATCH_DEBOUNCE_MS")?,
        redaction_enabled: parse_environment(environment, "SESSIONMESH_REDACTION_ENABLED")?,
        embedding_endpoint: environment.get("SESSIONMESH_EMBEDDING_ENDPOINT").cloned(),
        llm_endpoint: environment.get("SESSIONMESH_LLM_ENDPOINT").cloned(),
        model: environment.get("SESSIONMESH_MODEL").cloned(),
        token_budget: parse_environment(environment, "SESSIONMESH_TOKEN_BUDGET")?,
        correlation_threshold: parse_environment(environment, "SESSIONMESH_CORRELATION_THRESHOLD")?,
        source_mappings: None,
    })
}

fn parse_environment<T>(
    environment: &BTreeMap<String, String>,
    name: &str,
) -> Result<Option<T>, ConfigError>
where
    T: std::str::FromStr,
    T::Err: Display,
{
    environment
        .get(name)
        .map(|value| {
            value.parse::<T>().map_err(|error| {
                ConfigError::new(name, ConfigSource::Environment, error.to_string())
            })
        })
        .transpose()
}

fn resolve_path(
    raw: Resolved<PathBuf>,
    field: &str,
    user_home: &Path,
    environment: &BTreeMap<String, String>,
) -> Result<Resolved<PathBuf>, ConfigError> {
    let Resolved {
        value: raw_value,
        source,
    } = raw;
    let value = expand_path(&raw_value, user_home, environment)
        .map_err(|message| ConfigError::new(field, source, message))?;
    Ok(Resolved::new(value, source))
}

fn resolve_optional_path(
    raw: Resolved<Option<PathBuf>>,
    default: PathBuf,
    field: &str,
    user_home: &Path,
    environment: &BTreeMap<String, String>,
) -> Result<Resolved<PathBuf>, ConfigError> {
    let (path, source) = raw
        .value
        .map_or((default, ConfigSource::Default), |path| (path, raw.source));
    resolve_path(Resolved::new(path, source), field, user_home, environment)
}

fn resolve_source_mappings(
    raw: Resolved<Vec<SourceMapping>>,
    user_home: &Path,
    environment: &BTreeMap<String, String>,
) -> Result<Resolved<Vec<SourceMapping>>, ConfigError> {
    let mut mappings = Vec::with_capacity(raw.value.len());
    for (index, mapping) in raw.value.into_iter().enumerate() {
        let readable_path =
            expand_path(&mapping.readable_path, user_home, environment).map_err(|message| {
                ConfigError::new(
                    format!("source_mappings[{index}].readable_path"),
                    raw.source,
                    message,
                )
            })?;
        let original_path =
            expand_path(&mapping.original_path, user_home, environment).map_err(|message| {
                ConfigError::new(
                    format!("source_mappings[{index}].original_path"),
                    raw.source,
                    message,
                )
            })?;
        mappings.push(SourceMapping {
            readable_path,
            original_path,
        });
    }
    Ok(Resolved::new(mappings, raw.source))
}

fn resolve_endpoint(
    raw: Resolved<Option<String>>,
    field: &str,
    allow_network: bool,
) -> Result<Resolved<Option<Url>>, ConfigError> {
    let value = raw
        .value
        .map(|value| {
            let url = Url::parse(&value)
                .map_err(|error| ConfigError::new(field, raw.source, error.to_string()))?;
            if !matches!(url.scheme(), "http" | "https") {
                return Err(ConfigError::new(
                    field,
                    raw.source,
                    "only http and https endpoints are supported",
                ));
            }
            let is_loopback = url
                .host_str()
                .is_some_and(|host| matches!(host, "localhost" | "127.0.0.1" | "::1"));
            if !is_loopback && !allow_network {
                return Err(ConfigError::new(
                    field,
                    raw.source,
                    "remote endpoints require allow_network = true",
                ));
            }
            Ok(url)
        })
        .transpose()?;
    Ok(Resolved::new(value, raw.source))
}

fn expand_path(
    path: &Path,
    user_home: &Path,
    environment: &BTreeMap<String, String>,
) -> Result<PathBuf, String> {
    let raw = path
        .to_str()
        .ok_or_else(|| "path must contain valid UTF-8 for expansion".to_owned())?;
    let with_home = if raw == "~" {
        user_home.to_string_lossy().into_owned()
    } else if let Some(rest) = raw.strip_prefix("~/") {
        user_home.join(rest).to_string_lossy().into_owned()
    } else if raw.starts_with('~') {
        return Err("only ~ and ~/... home expansion are supported".to_owned());
    } else {
        raw.to_owned()
    };
    expand_environment_variables(&with_home, environment).map(PathBuf::from)
}

fn expand_environment_variables(
    value: &str,
    environment: &BTreeMap<String, String>,
) -> Result<String, String> {
    let mut result = String::with_capacity(value.len());
    let mut characters = value.char_indices().peekable();
    while let Some((_, character)) = characters.next() {
        if character != '$' {
            result.push(character);
            continue;
        }

        let Some(&(_, next)) = characters.peek() else {
            return Err("a trailing $ is not a valid variable reference".to_owned());
        };
        let name = if next == '{' {
            characters.next();
            let mut name = String::new();
            let mut closed = false;
            for (_, variable_character) in characters.by_ref() {
                if variable_character == '}' {
                    closed = true;
                    break;
                }
                name.push(variable_character);
            }
            if !closed {
                return Err("an environment variable reference is missing }".to_owned());
            }
            name
        } else {
            let mut name = String::new();
            while let Some(&(_, variable_character)) = characters.peek() {
                if variable_character == '_' || variable_character.is_ascii_alphanumeric() {
                    name.push(variable_character);
                    characters.next();
                } else {
                    break;
                }
            }
            name
        };

        if name.is_empty()
            || !name
                .chars()
                .next()
                .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        {
            return Err("invalid environment variable reference".to_owned());
        }
        let replacement = environment
            .get(&name)
            .ok_or_else(|| format!("environment variable {name} is not defined"))?;
        result.push_str(replacement);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs<'a>(
        home: &'a Path,
        user_toml: Option<&'a str>,
        environment: &'a BTreeMap<String, String>,
        cli: ConfigLayer,
    ) -> ConfigInputs<'a> {
        ConfigInputs {
            user_home: home,
            platform_data_home: None,
            user_toml,
            environment,
            cli,
        }
    }

    #[test]
    fn secure_defaults_are_local_and_owned_by_sessionmesh() {
        let environment = BTreeMap::new();
        let config = resolve(inputs(
            Path::new("/isolated/home"),
            None,
            &environment,
            ConfigLayer::default(),
        ))
        .expect("secure defaults should resolve");

        assert_eq!(
            config.home.value,
            PathBuf::from("/isolated/home/.local/share/sessionmesh")
        );
        assert_eq!(config.bind_address.value, IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert!(!config.allow_network.value);
        assert!(config.redaction_enabled.value);
        assert_eq!(
            config.database_path.value,
            config.home.value.join("sessionmesh.db")
        );
        assert_eq!(
            config.blob_store_path.value,
            config.home.value.join("blobs")
        );
    }

    #[test]
    fn applies_default_user_environment_and_cli_precedence() {
        let user = r"
            port = 8800
            token_budget = 2000
            correlation_threshold = 0.7
        ";
        let environment = BTreeMap::from([
            ("SESSIONMESH_PORT".to_owned(), "8900".to_owned()),
            ("SESSIONMESH_TOKEN_BUDGET".to_owned(), "3000".to_owned()),
        ]);
        let cli = ConfigLayer {
            port: Some(9000),
            ..ConfigLayer::default()
        };

        let config = resolve(inputs(
            Path::new("/isolated/home"),
            Some(user),
            &environment,
            cli,
        ))
        .expect("valid layers should resolve");

        assert_eq!(config.port, Resolved::new(9000, ConfigSource::Cli));
        assert_eq!(
            config.token_budget,
            Resolved::new(3000, ConfigSource::Environment)
        );
        assert_eq!(
            config.correlation_threshold,
            Resolved::new(0.7, ConfigSource::User)
        );
        assert_eq!(
            config.watch_debounce_ms,
            Resolved::new(DEFAULT_DEBOUNCE_MS, ConfigSource::Default)
        );
    }

    #[test]
    fn invalid_high_precedence_value_does_not_fall_back() {
        let environment = BTreeMap::from([(
            "SESSIONMESH_TOKEN_BUDGET".to_owned(),
            "not-a-number".to_owned(),
        )]);
        let error = resolve(inputs(
            Path::new("/isolated/home"),
            Some("token_budget = 2000"),
            &environment,
            ConfigLayer::default(),
        ))
        .expect_err("invalid environment value must fail");

        assert_eq!(error.field, "SESSIONMESH_TOKEN_BUDGET");
        assert_eq!(error.source, ConfigSource::Environment);
    }

    #[test]
    fn expands_home_and_documented_environment_variables_without_shell_syntax() {
        let environment = BTreeMap::from([("DATA_ROOT".to_owned(), "/data".to_owned())]);
        let cli = ConfigLayer {
            home: Some(PathBuf::from("~/state")),
            database_path: Some(PathBuf::from("${DATA_ROOT}/db/sessionmesh.db")),
            ..ConfigLayer::default()
        };

        let config = resolve(inputs(Path::new("/isolated/home"), None, &environment, cli))
            .expect("documented path expressions should resolve");

        assert_eq!(config.home.value, PathBuf::from("/isolated/home/state"));
        assert_eq!(
            config.database_path.value,
            PathBuf::from("/data/db/sessionmesh.db")
        );
    }

    #[test]
    fn rejects_shell_command_substitution_in_paths() {
        let environment = BTreeMap::new();
        let cli = ConfigLayer {
            database_path: Some(PathBuf::from("$(whoami)/sessionmesh.db")),
            ..ConfigLayer::default()
        };

        let error = resolve(inputs(Path::new("/isolated/home"), None, &environment, cli))
            .expect_err("shell syntax must not be accepted as a variable");

        assert_eq!(error.field, "database_path");
        assert_eq!(error.source, ConfigSource::Cli);
    }

    #[test]
    fn preserves_container_readable_and_host_origin_paths() {
        let environment = BTreeMap::new();
        let user = r#"
            [[source_mappings]]
            readable_path = "/sources/codex"
            original_path = "~/.codex"
        "#;

        let config = resolve(inputs(
            Path::new("/isolated/host-home"),
            Some(user),
            &environment,
            ConfigLayer::default(),
        ))
        .expect("source mapping should resolve");

        assert_eq!(
            config.source_mappings.value,
            vec![SourceMapping {
                readable_path: PathBuf::from("/sources/codex"),
                original_path: PathBuf::from("/isolated/host-home/.codex"),
            }]
        );
        assert_eq!(config.source_mappings.source, ConfigSource::User);
    }

    #[test]
    fn blocks_non_loopback_binding_without_explicit_network_permission() {
        let environment = BTreeMap::new();
        let cli = ConfigLayer {
            bind_address: Some(IpAddr::V4(Ipv4Addr::UNSPECIFIED)),
            ..ConfigLayer::default()
        };

        let error = resolve(inputs(Path::new("/isolated/home"), None, &environment, cli))
            .expect_err("public binding requires explicit permission");

        assert_eq!(error.field, "bind_address");
        assert_eq!(error.source, ConfigSource::Cli);
    }

    #[test]
    fn rejects_zero_port_at_the_layer_that_supplied_it() {
        let environment = BTreeMap::new();
        let cli = ConfigLayer {
            port: Some(0),
            ..ConfigLayer::default()
        };

        let error = resolve(inputs(Path::new("/isolated/home"), None, &environment, cli))
            .expect_err("port zero cannot accept client traffic");

        assert_eq!(error.field, "port");
        assert_eq!(error.source, ConfigSource::Cli);
    }

    #[test]
    fn container_home_environment_rebases_owned_default_paths() {
        let environment = BTreeMap::from([(
            "SESSIONMESH_HOME".to_owned(),
            "/var/lib/sessionmesh".to_owned(),
        )]);

        let config = resolve(inputs(
            Path::new("/isolated/home"),
            None,
            &environment,
            ConfigLayer::default(),
        ))
        .expect("container home should resolve");

        assert_eq!(
            config.home,
            Resolved::new(
                PathBuf::from("/var/lib/sessionmesh"),
                ConfigSource::Environment
            )
        );
        assert_eq!(
            config.database_path.value,
            PathBuf::from("/var/lib/sessionmesh/sessionmesh.db")
        );
        assert_eq!(
            config.blob_store_path.value,
            PathBuf::from("/var/lib/sessionmesh/blobs")
        );
    }

    #[test]
    fn rejects_remote_model_endpoint_when_network_access_is_disabled() {
        let environment = BTreeMap::new();
        let cli = ConfigLayer {
            llm_endpoint: Some("https://models.example.test/v1".to_owned()),
            ..ConfigLayer::default()
        };

        let error = resolve(inputs(Path::new("/isolated/home"), None, &environment, cli))
            .expect_err("remote endpoints require opt-in");

        assert_eq!(error.field, "llm_endpoint");
        assert_eq!(error.source, ConfigSource::Cli);
    }

    #[test]
    fn missing_user_home_is_reported_without_reading_the_process_home() {
        let environment = BTreeMap::new();
        let error = resolve(inputs(
            Path::new(""),
            None,
            &environment,
            ConfigLayer::default(),
        ))
        .expect_err("missing home must be explicit");

        assert_eq!(error.field, "user_home");
        assert_eq!(error.source, ConfigSource::Default);
    }

    #[test]
    fn unknown_user_configuration_field_is_rejected() {
        let environment = BTreeMap::new();
        let error = resolve(inputs(
            Path::new("/isolated/home"),
            Some("unexpected = true"),
            &environment,
            ConfigLayer::default(),
        ))
        .expect_err("unknown user fields must not be ignored");

        assert_eq!(error.field, "user_config");
        assert_eq!(error.source, ConfigSource::User);
    }
}
