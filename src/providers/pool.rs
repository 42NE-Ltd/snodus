use std::sync::Arc;

use super::{Provider, ProviderError};

#[derive(Clone)]
pub struct ProviderPool {
    providers: Vec<Arc<dyn Provider>>,
}

impl ProviderPool {
    pub fn new() -> Self {
        Self {
            providers: Vec::new(),
        }
    }

    pub fn register(&mut self, provider: Arc<dyn Provider>) {
        self.providers.push(provider);
    }

    pub fn len(&self) -> usize {
        self.providers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }

    pub fn resolve(&self, model: &str) -> Result<Arc<dyn Provider>, ProviderError> {
        self.providers
            .iter()
            .find(|p| p.is_enabled() && p.handles_model(model))
            .cloned()
            .ok_or_else(|| ProviderError::ModelNotSupported(model.into()))
    }

    pub fn providers(&self) -> &[Arc<dyn Provider>] {
        &self.providers
    }

    /// Get a provider by its unique name (case-sensitive).
    pub fn by_name(&self, name: &str) -> Option<Arc<dyn Provider>> {
        self.providers.iter().find(|p| p.name() == name).cloned()
    }
}

impl Default for ProviderPool {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::anthropic::AnthropicProvider;
    use crate::providers::openai::OpenAiProvider;

    #[test]
    fn resolves_to_first_matching_enabled_provider() {
        let mut pool = ProviderPool::new();
        pool.register(Arc::new(AnthropicProvider::new("sk-ant-api-fake".into())));
        pool.register(Arc::new(OpenAiProvider::new("sk-openai-fake".into())));

        let p = pool.resolve("claude-sonnet-4-5").unwrap();
        assert_eq!(p.name(), "anthropic");

        let p = pool.resolve("gpt-4o-mini").unwrap();
        assert_eq!(p.name(), "openai");
    }

    #[test]
    fn unknown_model_returns_error() {
        let mut pool = ProviderPool::new();
        pool.register(Arc::new(AnthropicProvider::new("sk-ant-fake".into())));
        match pool.resolve("foobar-7b") {
            Err(ProviderError::ModelNotSupported(_)) => {}
            _ => panic!("expected ModelNotSupported"),
        }
    }

    #[test]
    fn disabled_provider_is_skipped() {
        let mut pool = ProviderPool::new();
        // Empty api key → is_enabled() == false
        pool.register(Arc::new(AnthropicProvider::new(String::new())));
        match pool.resolve("claude-sonnet-4-5") {
            Err(ProviderError::ModelNotSupported(_)) => {}
            _ => panic!("expected ModelNotSupported for disabled provider"),
        }
    }
}
