use std::env;

fn bool_env(key: &str, default: bool) -> bool {
    match env::var(key) {
        Ok(v) => v == "1" || v.eq_ignore_ascii_case("true"),
        Err(_) => default,
    }
}

#[derive(Clone, Debug)]
pub struct GatewayConfig {
    pub host: String,
    pub port: u16,
    pub database_url: String,
    pub anthropic_api_key: String,
    pub openai_api_key: Option<String>,
    pub ollama_base_url: Option<String>,
    pub admin_username: String,
    pub admin_password: String,
    pub router_enabled: bool,
}

impl GatewayConfig {
    pub fn from_env() -> Result<Self, String> {
        Ok(Self {
            host: env::var("SNODUS_HOST").unwrap_or_else(|_| "0.0.0.0".into()),
            port: env::var("SNODUS_PORT")
                .unwrap_or_else(|_| "8080".into())
                .parse()
                .map_err(|_| "SNODUS_PORT must be a valid port number")?,
            database_url: env::var("DATABASE_URL").map_err(|_| "DATABASE_URL is required")?,
            anthropic_api_key: env::var("ANTHROPIC_API_KEY").unwrap_or_default(),
            openai_api_key: env::var("OPENAI_API_KEY").ok().filter(|s| !s.is_empty()),
            ollama_base_url: env::var("OLLAMA_BASE_URL").ok().filter(|s| !s.is_empty()),
            admin_username: env::var("ADMIN_USERNAME").unwrap_or_else(|_| "admin".into()),
            admin_password: env::var("ADMIN_PASSWORD").map_err(|_| "ADMIN_PASSWORD is required")?,
            router_enabled: bool_env("SNODUS_ROUTER_ENABLED", false),
        })
    }

    pub fn listen_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}
