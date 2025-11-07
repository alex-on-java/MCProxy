//! Port check client middleware that filters tools based on TCP port accessibility.
//!
//! This middleware is useful for servers that require external services (like browsers)
//! to be running. It dynamically shows/hides tools based on whether a TCP port is accessible.

use async_trait::async_trait;
use rmcp::model::ListToolsResult;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tracing::{debug, info};
use uuid::Uuid;

use crate::error::{ProxyError, Result};
use crate::middleware::client::ClientMiddleware;
use crate::middleware::ClientMiddlewareFactory;

/// Configuration for port check middleware
#[derive(Debug, Clone, Deserialize)]
pub struct PortCheckConfig {
    /// Hostname or IP address to check
    pub host: String,

    /// TCP port to check
    pub port: u16,

    /// Connection timeout in milliseconds (default: 100ms)
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
}

fn default_timeout_ms() -> u64 {
    100
}

impl Default for PortCheckConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 0,
            timeout_ms: default_timeout_ms(),
        }
    }
}

/// Client middleware that filters tools based on TCP port accessibility.
///
/// When the configured port is not accessible (connection fails), all tools from
/// this server are hidden. When the port becomes accessible, tools are shown normally.
///
/// This is useful for servers like chrome-devtools-mcp that expose tools regardless
/// of whether the underlying service (Chrome) is running.
#[derive(Debug)]
pub struct PortCheckClientMiddleware {
    config: PortCheckConfig,
    server_name: String,
}

impl PortCheckClientMiddleware {
    /// Creates a new PortCheckClientMiddleware with the given configuration
    pub fn new(server_name: String, config: PortCheckConfig) -> Result<Self> {
        if config.port == 0 {
            return Err(ProxyError::config("Port check middleware requires a non-zero port"));
        }

        Ok(Self {
            config,
            server_name,
        })
    }

    /// Check if the configured port is accessible
    async fn is_port_accessible(&self) -> bool {
        let addr = format!("{}:{}", self.config.host, self.config.port);
        let timeout = Duration::from_millis(self.config.timeout_ms);

        debug!("[{}] Checking port accessibility: {}", self.server_name, addr);

        match tokio::time::timeout(timeout, TcpStream::connect(&addr)).await {
            Ok(Ok(_)) => {
                debug!("[{}] Port {} is accessible", self.server_name, addr);
                true
            }
            Ok(Err(e)) => {
                debug!("[{}] Port {} is not accessible: {}", self.server_name, addr, e);
                false
            }
            Err(_) => {
                debug!("[{}] Port {} check timed out after {}ms",
                       self.server_name, addr, self.config.timeout_ms);
                false
            }
        }
    }
}

#[async_trait]
impl ClientMiddleware for PortCheckClientMiddleware {
    async fn modify_list_tools_result(&self, _request_id: Uuid, result: &mut ListToolsResult) {
        let is_accessible = self.is_port_accessible().await;

        if !is_accessible {
            let tool_count = result.tools.len();
            if tool_count > 0 {
                info!("🔌 [{}] Port {}:{} not accessible - hiding {} tool(s)",
                      self.server_name, self.config.host, self.config.port, tool_count);
                result.tools.clear();
            }
        } else {
            let tool_count = result.tools.len();
            if tool_count > 0 {
                debug!("✅ [{}] Port {}:{} accessible - exposing {} tool(s)",
                       self.server_name, self.config.host, self.config.port, tool_count);
            }
        }
    }
}

/// Factory for creating PortCheckClientMiddleware from configuration
#[derive(Debug)]
pub struct PortCheckClientFactory;

impl ClientMiddlewareFactory for PortCheckClientFactory {
    fn create(&self, server_name: &str, config: &serde_json::Value) -> Result<Arc<dyn ClientMiddleware>> {
        let port_config: PortCheckConfig = if config.is_null() {
            return Err(ProxyError::config(
                "Port check middleware requires configuration with 'host' and 'port'"
            ));
        } else {
            serde_json::from_value(config.clone()).map_err(|e| {
                ProxyError::config(format!("Invalid port check configuration: {}", e))
            })?
        };

        let middleware = PortCheckClientMiddleware::new(server_name.to_string(), port_config)?;
        Ok(Arc::new(middleware))
    }

    fn middleware_type(&self) -> &'static str {
        "port_check"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::Tool;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_port_check_inaccessible_port() {
        // Use a port that's very unlikely to be in use
        let config = PortCheckConfig {
            host: "localhost".to_string(),
            port: 59999,
            timeout_ms: 50,
        };
        let middleware = PortCheckClientMiddleware::new("test-server".to_string(), config).unwrap();

        let mut result = ListToolsResult {
            tools: vec![
                Tool {
                    name: "test_tool".into(),
                    description: None,
                    input_schema: Arc::new(serde_json::Map::new()),
                    annotations: None,
                },
            ],
            next_cursor: None,
        };

        middleware.modify_list_tools_result(Uuid::new_v4(), &mut result).await;

        // Port should be inaccessible, so tools should be cleared
        assert_eq!(result.tools.len(), 0);
    }

    #[tokio::test]
    async fn test_port_check_accessible_port() {
        // Start a simple TCP listener
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let config = PortCheckConfig {
            host: "127.0.0.1".to_string(),
            port,
            timeout_ms: 100,
        };
        let middleware = PortCheckClientMiddleware::new("test-server".to_string(), config).unwrap();

        let mut result = ListToolsResult {
            tools: vec![
                Tool {
                    name: "test_tool".into(),
                    description: None,
                    input_schema: Arc::new(serde_json::Map::new()),
                    annotations: None,
                },
            ],
            next_cursor: None,
        };

        middleware.modify_list_tools_result(Uuid::new_v4(), &mut result).await;

        // Port should be accessible, so tools should remain
        assert_eq!(result.tools.len(), 1);
        assert_eq!(result.tools[0].name.as_ref(), "test_tool");
    }

    #[test]
    fn test_config_requires_port() {
        let config = PortCheckConfig {
            host: "localhost".to_string(),
            port: 0,
            timeout_ms: 100,
        };

        let result = PortCheckClientMiddleware::new("test-server".to_string(), config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("non-zero port"));
    }

    #[test]
    fn test_factory_requires_config() {
        let factory = PortCheckClientFactory;
        let result = factory.create("test-server", &serde_json::Value::Null);
        assert!(result.is_err());
    }

    #[test]
    fn test_factory_creates_middleware() {
        let factory = PortCheckClientFactory;
        let config = serde_json::json!({
            "host": "localhost",
            "port": 9222
        });

        let result = factory.create("test-server", &config);
        assert!(result.is_ok());
    }
}
