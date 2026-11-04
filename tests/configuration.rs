//! Provider selection must never mix credentials or connection settings.

use llm_mcp::{
    config::{Config, ServiceProvider},
    provider::Provider,
};

fn config(settings: &[(&str, &str)]) -> llm_mcp::error::Result<Config> {
    Config::from_lookup(|key| {
        settings
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| (*value).into())
    })
}

#[test]
fn provider_defaults_and_credentials_are_isolated() {
    let openai = config(&[("OPENROUTER_API_KEY", "router-secret")]).unwrap();
    assert_eq!(openai.provider, ServiceProvider::OpenAI);
    assert_eq!(openai.base_url.as_str(), "https://api.openai.com/v1/");
    assert!(
        Provider::new(openai)
            .err()
            .unwrap()
            .message
            .contains("OPENAI_API_KEY")
    );
    let router = config(&[
        ("LLM_MCP_PROVIDER", "openrouter"),
        ("OPENAI_API_KEY", "openai-secret"),
        ("OPENAI_BASE_URL", "not a URL"),
        ("OPENAI_ORGANIZATION", "org"),
        ("OPENAI_PROJECT", "project"),
    ])
    .unwrap();
    assert_eq!(router.base_url.as_str(), "https://openrouter.ai/api/v1/");
    assert!(router.organization.is_none() && router.project.is_none());
    assert!(
        Provider::new(router)
            .err()
            .unwrap()
            .message
            .contains("OPENROUTER_API_KEY")
    );
    let router = config(&[
        ("LLM_MCP_PROVIDER", "openrouter"),
        ("OPENROUTER_API_KEY", "router-secret"),
        ("OPENROUTER_BASE_URL", "http://localhost:1234/custom/v1"),
    ])
    .unwrap();
    assert_eq!(router.api_key.as_deref(), Some("router-secret"));
    assert_eq!(router.base_url.as_str(), "http://localhost:1234/custom/v1/");
    assert!(!format!("{router:?}").contains("secret"));
}

#[test]
fn invalid_provider_auth_and_urls_fail_without_echoing_values() {
    for settings in [
        vec![("LLM_MCP_PROVIDER", "unknown")],
        vec![
            ("LLM_MCP_PROVIDER", "openrouter"),
            ("LLM_MCP_AUTH", "oauth"),
        ],
    ] {
        assert_eq!(config(&settings).unwrap_err().code, "configuration");
    }
    for url in [
        "invalid-secret",
        "ftp://host/",
        "https://user:secret@host/",
        "https://host/?secret",
        "https://host/#secret",
    ] {
        let error = config(&[
            ("LLM_MCP_PROVIDER", "openrouter"),
            ("OPENROUTER_BASE_URL", url),
        ])
        .unwrap_err();
        assert_eq!(error.code, "configuration");
        assert!(error.message.contains("OPENROUTER_BASE_URL"));
        assert!(!error.message.contains("secret"));
    }
}
