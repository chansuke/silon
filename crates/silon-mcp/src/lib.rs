//! MCP server for Silon apps.
//!
//! [`SilonMcp`] exposes the introspection HTTP API of a running app (started
//! with `--features introspect`) as MCP tools. It holds no state of its own:
//! every tool call is one HTTP request.

use reqwest::{Client, Url};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{Implementation, ServerCapabilities, ServerConfig};
use rmcp::{ServerHandler, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

/// Where `silon-introspect` listens unless the app says otherwise.
pub const DEFAULT_URL: &str = "http://127.0.0.1:3737";

const INSTRUCTIONS: &str = "Reads and drives a running Silon app. Call list_features \
first: it names each feature and gives the JSON Schema of its actions. dispatch_action \
returns the state right after the action; effects such as timers finish later, so read \
get_state again to see their results. get_history lists the recent actions (up to 256) \
in order, and replay recomputes the state from them without changing the app.";

#[derive(Clone)]
pub struct SilonMcp {
    client: Client,
    base: Url,
    tool_router: ToolRouter<Self>,
}

#[derive(Deserialize, JsonSchema)]
pub struct FeatureParams {
    /// A feature name from list_features, e.g. "counter".
    pub feature: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct DispatchParams {
    /// A feature name from list_features, e.g. "counter".
    pub feature: String,
    /// The action as JSON, valid against the feature's schema from
    /// list_features, e.g. "Increment" or {"DraftChanged": "milk"}.
    pub action: Value,
}

#[derive(Deserialize, JsonSchema)]
pub struct ReplayParams {
    /// A feature name from list_features, e.g. "counter".
    pub feature: String,
    /// The `seq` of the last history entry to replay. Omit it to replay all.
    pub through: Option<u64>,
}

#[tool_router(router = tool_router)]
impl SilonMcp {
    /// Talks to the introspection API at `base`, e.g. [`DEFAULT_URL`].
    pub fn new(base: Url) -> reqwest::Result<Self> {
        Ok(Self {
            client: Client::builder().no_proxy().build()?,
            base,
            tool_router: Self::tool_router(),
        })
    }

    #[tool(description = "List the app's features with the JSON Schema of their actions.")]
    async fn list_features(&self) -> Result<String, String> {
        self.send(self.client.get(self.url(&["features"]))).await
    }

    #[tool(description = "Return the current state of a feature as JSON.")]
    async fn get_state(
        &self,
        Parameters(FeatureParams { feature }): Parameters<FeatureParams>,
    ) -> Result<String, String> {
        self.send(self.client.get(self.url(&["state", &feature])))
            .await
    }

    #[tool(
        description = "Dispatch an action to a feature, as if the user did it, and return the state right after it. The window updates too."
    )]
    async fn dispatch_action(
        &self,
        Parameters(DispatchParams { feature, action }): Parameters<DispatchParams>,
    ) -> Result<String, String> {
        let request = self.client.post(self.url(&["dispatch", &feature]));
        self.send(request.json(&action)).await
    }

    #[tool(
        description = "Return the recent actions of a feature (up to 256), oldest first: {base, dropped, entries: [{seq, action}]}. Applying the entries to base gives the current state."
    )]
    async fn get_history(
        &self,
        Parameters(FeatureParams { feature }): Parameters<FeatureParams>,
    ) -> Result<String, String> {
        self.send(self.client.get(self.url(&["history", &feature])))
            .await
    }

    #[tool(
        description = "Recompute a feature's state from its history, up to and including entry `through` (all entries if omitted), without changing the app. Returns {state, through, matches_current}."
    )]
    async fn replay(
        &self,
        Parameters(ReplayParams { feature, through }): Parameters<ReplayParams>,
    ) -> Result<String, String> {
        let mut url = self.url(&["replay", &feature]);
        if let Some(through) = through {
            url.query_pairs_mut()
                .append_pair("through", &through.to_string());
        }
        self.send(self.client.get(url)).await
    }
}

impl SilonMcp {
    /// `base` followed by `segments`, each percent-encoded as one segment.
    fn url(&self, segments: &[&str]) -> Url {
        let mut url = self.base.clone();
        if let Ok(mut path) = url.path_segments_mut() {
            path.pop_if_empty().extend(segments);
        }
        url
    }

    /// Sends `request` and returns the JSON body, pretty-printed, or the
    /// API's error message.
    async fn send(&self, request: reqwest::RequestBuilder) -> Result<String, String> {
        let response = request.send().await.map_err(|err| {
            format!(
                "cannot reach the app at {}: {err}. Is it running with --features introspect?",
                self.base
            )
        })?;
        let status = response.status();
        let body: Value = response
            .json()
            .await
            .map_err(|err| format!("the app answered {status} without JSON: {err}"))?;
        if status.is_success() {
            Ok(serde_json::to_string_pretty(&body).unwrap_or_else(|_| body.to_string()))
        } else {
            let message = body["error"]
                .as_str()
                .map_or(body.to_string(), str::to_owned);
            Err(format!("{status}: {message}"))
        }
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for SilonMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(INSTRUCTIONS)
    }
}
