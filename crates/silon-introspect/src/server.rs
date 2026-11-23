//! Local HTTP API over an [`IntrospectionRegistry`].
//!
//! - `GET /features` lists registered features with their action schemas.
//! - `GET /state/{name}` returns a feature's state.
//! - `POST /dispatch/{name}` dispatches the JSON body as an action and returns
//!   the updated state.

use std::net::{SocketAddr, TcpListener};
use std::thread::JoinHandle;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Value, json};

use crate::{IntrospectionRegistry, StoreHandle};

/// Loopback address the server binds to unless told otherwise.
pub const DEFAULT_ADDR: SocketAddr =
    SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), 3737);

pub fn router(registry: IntrospectionRegistry) -> Router {
    Router::new()
        .route("/features", get(list_features))
        .route("/state/{name}", get(get_state))
        .route("/dispatch/{name}", post(dispatch))
        .with_state(registry)
}

/// Binds `addr` now, so that errors such as a taken port surface to the
/// caller, then serves on a dedicated thread with its own Tokio runtime.
pub fn spawn_server(
    registry: IntrospectionRegistry,
    addr: SocketAddr,
) -> std::io::Result<JoinHandle<()>> {
    let listener = TcpListener::bind(addr)?;
    listener.set_nonblocking(true)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .thread_name("silon-introspect")
        .enable_io()
        .build()?;

    std::thread::Builder::new()
        .name("silon-introspect".into())
        .spawn(move || {
            runtime.block_on(async move {
                let listener = match tokio::net::TcpListener::from_std(listener) {
                    Ok(listener) => listener,
                    Err(err) => return eprintln!("silon-introspect: {err}"),
                };
                if let Err(err) = axum::serve(listener, router(registry)).await {
                    eprintln!("silon-introspect: server stopped: {err}");
                }
            });
        })
}

async fn list_features(
    State(registry): State<IntrospectionRegistry>,
) -> Result<Json<Value>, ApiError> {
    let mut features = Vec::new();
    for handle in registry.handles() {
        let schema = handle.get_schema().await.map_err(ApiError::unavailable)?;
        features.push(json!({ "name": handle.name(), "schema": schema }));
    }
    Ok(Json(Value::Array(features)))
}

async fn get_state(
    State(registry): State<IntrospectionRegistry>,
    Path(name): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let state = find(&registry, &name)?
        .get_state()
        .await
        .map_err(ApiError::unavailable)?;
    Ok(Json(state))
}

async fn dispatch(
    State(registry): State<IntrospectionRegistry>,
    Path(name): Path<String>,
    Json(action): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let state = find(&registry, &name)?
        .dispatch_json(action)
        .await
        .map_err(ApiError::bad_request)?;
    Ok(Json(state))
}

fn find(registry: &IntrospectionRegistry, name: &str) -> Result<StoreHandle, ApiError> {
    registry.get(name).ok_or_else(|| ApiError {
        status: StatusCode::NOT_FOUND,
        message: format!("unknown feature `{name}`"),
    })
}

/// An error answered as `{"error": message}` with `status`.
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn unavailable(message: String) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message,
        }
    }

    fn bad_request(message: String) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::fake_store;
    use axum::body::{Body, to_bytes};
    use axum::http::Request;
    use tower::ServiceExt;

    fn registry() -> IntrospectionRegistry {
        let registry = IntrospectionRegistry::default();
        registry.register(fake_store("counter"));
        registry
    }

    async fn call(router: Router, request: Request<Body>) -> (StatusCode, Value) {
        let response = router.oneshot(request).await.unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, serde_json::from_slice(&body).unwrap())
    }

    fn get(uri: &str) -> Request<Body> {
        Request::get(uri).body(Body::empty()).unwrap()
    }

    fn post(uri: &str, body: &str) -> Request<Body> {
        Request::post(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_owned()))
            .unwrap()
    }

    #[tokio::test]
    async fn features_lists_names_and_schemas() {
        let (status, body) = call(router(registry()), get("/features")).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body,
            json!([{ "name": "counter", "schema": { "enum": ["Increment"] } }])
        );
    }

    #[tokio::test]
    async fn state_and_dispatch_reach_the_store() {
        let app = router(registry());

        let (status, body) = call(app.clone(), get("/state/counter")).await;
        assert_eq!((status, body), (StatusCode::OK, json!({ "count": 0 })));

        let (status, body) = call(app.clone(), post("/dispatch/counter", r#""Increment""#)).await;
        assert_eq!((status, body), (StatusCode::OK, json!({ "count": 1 })));

        let (_, body) = call(app, get("/state/counter")).await;
        assert_eq!(body, json!({ "count": 1 }));
    }

    #[tokio::test]
    async fn rejected_action_is_a_bad_request() {
        let (status, body) = call(router(registry()), post("/dispatch/counter", r#""Nope""#)).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body, json!({ "error": r#"unknown action: "Nope""# }));
    }

    #[tokio::test]
    async fn unknown_feature_is_not_found() {
        let app = router(registry());

        let (status, body) = call(app.clone(), get("/state/missing")).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, json!({ "error": "unknown feature `missing`" }));

        let (status, _) = call(app, post("/dispatch/missing", r#""Increment""#)).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}
