use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use std::fs;
use rand::Rng;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

static CONFIG_MUTEX: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_server")]
    pub server: ServerConfig,
    #[serde(default)]
    pub anthropic_direct: AnthropicDirect,
    #[serde(default)]
    pub providers: Vec<Provider>,
    #[serde(default)]
    pub model_mappings: Vec<ModelMapping>,
    #[serde(default)]
    pub default_provider_id: String,
    #[serde(default)]
    pub default_model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "generate_api_key")]
    pub api_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicDirect {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_anthropic_url")]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,
    #[serde(rename = "type", default = "default_openai")]
    pub provider_type: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelMapping {
    pub claude_model: String,
    pub provider_id: String,
    pub target_model: String,
}

fn default_port() -> u16 { 8000 }
fn default_host() -> String { "0.0.0.0".to_string() }
fn default_openai() -> String { "openai".to_string() }
fn default_true() -> bool { true }
fn default_anthropic_url() -> String { "https://api.anthropic.com".to_string() }

fn generate_api_key() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn default_server() -> ServerConfig {
    ServerConfig {
        port: default_port(),
        host: default_host(),
        api_key: generate_api_key(),
    }
}

impl Default for AnthropicDirect {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: default_anthropic_url(),
            api_key: String::new(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: default_server(),
            anthropic_direct: AnthropicDirect::default(),
            providers: Vec::new(),
            model_mappings: Vec::new(),
            default_provider_id: String::new(),
            default_model: String::new(),
        }
    }
}

pub fn get_config_path() -> PathBuf {
    if let Ok(env_path) = std::env::var("CONFIG_PATH") {
        return PathBuf::from(env_path);
    }
    let exe_dir = std::env::current_exe()
        .map(|p| p.parent().unwrap_or_else(|| std::path::Path::new(".")).to_path_buf())
        .unwrap_or_else(|_| PathBuf::from("."));
    exe_dir.join("config.json")
}

pub fn load_config() -> Config {
    let _lock = CONFIG_MUTEX.lock().unwrap();
    let path = get_config_path();
    if path.exists() {
        let data = fs::read_to_string(&path).unwrap_or_default();
        serde_json::from_str(&data).unwrap_or_default()
    } else {
        let config = Config::default();
        let _ = save_config_inner(&config);
        config
    }
}

fn save_config_inner(config: &Config) -> Result<(), std::io::Error> {
    let path = get_config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let data = serde_json::to_string_pretty(config).unwrap();
    fs::write(path, data)
}

pub fn save_config(config: &Config) {
    let _lock = CONFIG_MUTEX.lock().unwrap();
    let _ = save_config_inner(config);
}

pub fn new_provider_id() -> String {
    uuid::Uuid::new_v4().as_simple().to_string()
}
