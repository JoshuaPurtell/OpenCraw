use crate::config::{
    ControlApiKeyConfig, OpenShellConfig, RuntimeMode, normalize_control_api_scope,
};
use axum::Json;
use axum::body::Body;
use axum::http::header::AUTHORIZATION;
use axum::http::{HeaderMap, Method, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ScopedControlApiToken {
    pub token: String,
    pub scopes: Vec<String>,
}

impl ScopedControlApiToken {
    fn from_config(config: &ControlApiKeyConfig) -> Option<Self> {
        let token = config.token.trim();
        if token.is_empty() {
            return None;
        }
        let mut scopes = Vec::new();
        for scope in &config.scopes {
            let Some(scope) = normalize_control_api_scope(scope) else {
                continue;
            };
            if !scopes.iter().any(|existing| existing == &scope) {
                scopes.push(scope);
            }
        }
        Some(Self {
            token: token.to_string(),
            scopes,
        })
    }

    fn grants_scope(&self, required_scope: &str) -> bool {
        // Empty scope list means full mutating control-plane access.
        if self.scopes.is_empty() {
            return true;
        }
        self.scopes
            .iter()
            .any(|scope| scope == "*" || scope == "control:write" || scope == required_scope)
    }
}

#[derive(Debug, Clone)]
pub struct MutatingAuthPolicy {
    pub require_auth_for_mutating: bool,
    pub allow_insecure_mutating_requests: bool,
    pub require_org_header_for_mutating: bool,
    pub control_api_tokens: Vec<ScopedControlApiToken>,
    pub mutating_auth_exempt_prefixes: Vec<String>,
}

impl Default for MutatingAuthPolicy {
    fn default() -> Self {
        Self {
            require_auth_for_mutating: true,
            allow_insecure_mutating_requests: false,
            require_org_header_for_mutating: true,
            control_api_tokens: Vec::new(),
            mutating_auth_exempt_prefixes: vec![
                "/api/v1/os/automation/webhook/".to_string(),
                "/api/v1/os/automation/poll/".to_string(),
            ],
        }
    }
}

impl MutatingAuthPolicy {
    pub fn from_config(cfg: &OpenShellConfig) -> Self {
        let control_api_tokens = cfg
            .control_api_key_pool()
            .iter()
            .filter_map(ScopedControlApiToken::from_config)
            .collect::<Vec<_>>();

        let strict_mutating_auth =
            cfg.runtime.mode == RuntimeMode::Prod || !control_api_tokens.is_empty();
        let mutating_auth_exempt_prefixes = cfg
            .security
            .mutating_auth_exempt_prefixes
            .iter()
            .map(|prefix| prefix.trim().to_string())
            .filter(|prefix| !prefix.is_empty())
            .collect::<Vec<_>>();

        Self {
            require_auth_for_mutating: strict_mutating_auth,
            allow_insecure_mutating_requests: !strict_mutating_auth,
            require_org_header_for_mutating: strict_mutating_auth,
            control_api_tokens,
            mutating_auth_exempt_prefixes,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MutatingAuthPolicyExt(pub MutatingAuthPolicy);

fn is_mutating(method: &Method) -> bool {
    matches!(
        *method,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    )
}

fn required_scope_for_mutating_path(path: &str) -> &'static str {
    if path.starts_with("/api/v1/os/config/") {
        return "config:write";
    }
    if path.starts_with("/api/v1/os/sessions/") {
        return "sessions:write";
    }
    if path.starts_with("/api/v1/os/automation/") {
        return "automation:write";
    }
    if path.starts_with("/api/v1/os/skills/") {
        return "skills:write";
    }
    if path.starts_with("/api/v1/os/messages/") {
        return "messages:write";
    }
    if path.starts_with("/api/v1/os/channels/") {
        return "channels:write";
    }
    "control:write"
}

fn parse_bearer_token(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get(AUTHORIZATION)?.to_str().ok()?;
    let mut parts = raw.trim().splitn(2, char::is_whitespace);
    let scheme = parts.next()?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = parts.next()?.trim();
    if token.is_empty() {
        return None;
    }
    Some(token.to_string())
}

fn path_matches_prefix(path: &str, prefix: &str) -> bool {
    if path == prefix {
        return true;
    }
    let Some(suffix) = path.strip_prefix(prefix) else {
        return false;
    };
    if prefix.ends_with('/') {
        return true;
    }
    suffix.starts_with('/')
}

fn is_mutating_path_exempt(path: &str, policy: &MutatingAuthPolicy) -> bool {
    policy
        .mutating_auth_exempt_prefixes
        .iter()
        .any(|prefix| path_matches_prefix(path, prefix))
}

fn find_control_api_token<'a>(
    provided_token: &str,
    policy: &'a MutatingAuthPolicy,
) -> Option<&'a ScopedControlApiToken> {
    policy
        .control_api_tokens
        .iter()
        .find(|token| token.token == provided_token)
}

fn valid_org_id_header(headers: &HeaderMap) -> bool {
    let Some(raw) = headers.get("x-org-id") else {
        return false;
    };
    let Ok(raw) = raw.to_str() else {
        return false;
    };
    Uuid::parse_str(raw.trim()).is_ok()
}

fn unauthorized(message: impl Into<String>) -> Response {
    unauthorized_with_context(None, None, message)
}

fn unauthorized_with_context(
    code: Option<&str>,
    required_scope: Option<&str>,
    message: impl Into<String>,
) -> Response {
    let message = message.into();
    if let Some(code) = code {
        tracing::warn!(error_code = code, ?required_scope, error = %message, "mutating auth rejected");
    }
    let mut body = serde_json::Map::new();
    body.insert("status".to_string(), json!("error"));
    body.insert("error".to_string(), json!(message));
    if let Some(code) = code {
        body.insert("error_code".to_string(), json!(code));
    }
    if let Some(required_scope) = required_scope {
        body.insert("required_scope".to_string(), json!(required_scope));
    }
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::Value::Object(body)),
    )
        .into_response()
}

#[tracing::instrument(level = "debug", skip_all)]
pub async fn require_mutating_auth(req: Request<Body>, next: Next) -> Response {
    if !is_mutating(req.method()) {
        return next.run(req).await;
    }

    let policy = req
        .extensions()
        .get::<MutatingAuthPolicyExt>()
        .map(|v| v.0.clone())
        .unwrap_or_default();
    if is_mutating_path_exempt(req.uri().path(), &policy) {
        return next.run(req).await;
    }

    if !policy.require_auth_for_mutating {
        return next.run(req).await;
    }

    if policy.require_org_header_for_mutating && !valid_org_id_header(req.headers()) {
        return unauthorized_with_context(
            Some("missing_or_invalid_org_id"),
            None,
            "missing or invalid x-org-id header",
        );
    }

    if !policy.control_api_tokens.is_empty() {
        let Some(provided) = parse_bearer_token(req.headers()) else {
            return unauthorized_with_context(
                Some("missing_bearer_token"),
                Some(required_scope_for_mutating_path(req.uri().path())),
                "missing bearer token",
            );
        };
        let Some(token) = find_control_api_token(&provided, &policy) else {
            return unauthorized_with_context(
                Some("invalid_bearer_token"),
                Some(required_scope_for_mutating_path(req.uri().path())),
                "invalid bearer token",
            );
        };
        let required_scope = required_scope_for_mutating_path(req.uri().path());
        if !token.grants_scope(required_scope) {
            return unauthorized_with_context(
                Some("missing_required_scope"),
                Some(required_scope),
                format!("bearer token missing required scope: {required_scope}"),
            );
        }
        return next.run(req).await;
    }

    if policy.allow_insecure_mutating_requests {
        return next.run(req).await;
    }

    unauthorized_with_context(
        Some("missing_control_api_auth_config"),
        Some(required_scope_for_mutating_path(req.uri().path())),
        "mutating requests require security.control_api_key or security.control_api_keys",
    )
}

#[cfg(test)]
mod tests {
    use super::{
        MutatingAuthPolicy, MutatingAuthPolicyExt, ScopedControlApiToken, find_control_api_token,
        is_mutating_path_exempt, parse_bearer_token, require_mutating_auth,
        required_scope_for_mutating_path, unauthorized, valid_org_id_header,
    };
    use axum::Router;
    use axum::body::{Body, to_bytes};
    use axum::http::header::AUTHORIZATION;
    use axum::http::{HeaderMap, Request, StatusCode};
    use axum::middleware;
    use axum::routing::post;
    use axum::{Extension, Json};
    use serde_json::json;
    use tower::util::ServiceExt;
    use uuid::Uuid;

    fn build_auth_test_router(policy: MutatingAuthPolicy) -> Router {
        Router::new()
            .route(
                "/api/v1/os/config/apply",
                post(|| async { Json(json!({ "status": "ok" })) }),
            )
            .route(
                "/api/v1/os/automation/webhook/github",
                post(|| async { Json(json!({ "status": "ok" })) }),
            )
            .layer(middleware::from_fn(require_mutating_auth))
            .layer(Extension(MutatingAuthPolicyExt(policy)))
    }

    async fn response_body_json(response: axum::response::Response) -> serde_json::Value {
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        serde_json::from_slice(&body).expect("response json")
    }

    #[test]
    fn valid_org_id_header_accepts_uuid() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-org-id",
            Uuid::new_v4().to_string().parse().expect("header value"),
        );
        assert!(valid_org_id_header(&headers));
    }

    #[test]
    fn valid_org_id_header_rejects_invalid_uuid() {
        let mut headers = HeaderMap::new();
        headers.insert("x-org-id", "not-a-uuid".parse().expect("header value"));
        assert!(!valid_org_id_header(&headers));
    }

    #[test]
    fn mutating_path_exempt_prefix_matches_webhook_and_poll() {
        let policy = MutatingAuthPolicy::default();
        assert!(is_mutating_path_exempt(
            "/api/v1/os/automation/webhook/github",
            &policy
        ));
        assert!(is_mutating_path_exempt(
            "/api/v1/os/automation/poll/github",
            &policy
        ));
        assert!(!is_mutating_path_exempt(
            "/api/v1/os/automation/jobs",
            &policy
        ));
    }

    #[test]
    fn mutating_path_exempt_prefix_requires_segment_boundary() {
        let policy = MutatingAuthPolicy {
            mutating_auth_exempt_prefixes: vec!["/api/v1/os/config".to_string()],
            ..MutatingAuthPolicy::default()
        };
        assert!(is_mutating_path_exempt("/api/v1/os/config", &policy));
        assert!(is_mutating_path_exempt("/api/v1/os/config/apply", &policy));
        assert!(!is_mutating_path_exempt(
            "/api/v1/os/configuration",
            &policy
        ));
    }

    #[test]
    fn scope_matrix_routes_to_expected_prefix_scope() {
        assert_eq!(
            required_scope_for_mutating_path("/api/v1/os/config/apply"),
            "config:write"
        );
        assert_eq!(
            required_scope_for_mutating_path("/api/v1/os/sessions/abc/model"),
            "sessions:write"
        );
        assert_eq!(
            required_scope_for_mutating_path("/api/v1/os/automation/jobs"),
            "automation:write"
        );
        assert_eq!(
            required_scope_for_mutating_path("/api/v1/os/skills/install"),
            "skills:write"
        );
        assert_eq!(
            required_scope_for_mutating_path("/api/v1/os/messages/send"),
            "messages:write"
        );
        assert_eq!(
            required_scope_for_mutating_path("/api/v1/os/channels/reconnect"),
            "channels:write"
        );
        assert_eq!(
            required_scope_for_mutating_path("/api/v1/os/memory/summarize"),
            "control:write"
        );
    }

    #[test]
    fn scoped_token_grants_expected_scope_fallbacks() {
        let wildcard = ScopedControlApiToken {
            token: "wild".to_string(),
            scopes: vec!["*".to_string()],
        };
        assert!(wildcard.grants_scope("skills:write"));

        let control = ScopedControlApiToken {
            token: "control".to_string(),
            scopes: vec!["control:write".to_string()],
        };
        assert!(control.grants_scope("messages:write"));

        let specific = ScopedControlApiToken {
            token: "specific".to_string(),
            scopes: vec!["sessions:write".to_string()],
        };
        assert!(specific.grants_scope("sessions:write"));
        assert!(!specific.grants_scope("config:write"));

        let full = ScopedControlApiToken {
            token: "full".to_string(),
            scopes: vec![],
        };
        assert!(full.grants_scope("automation:write"));
    }

    #[test]
    fn find_control_api_token_returns_matching_entry() {
        let policy = MutatingAuthPolicy {
            control_api_tokens: vec![
                ScopedControlApiToken {
                    token: "alpha".to_string(),
                    scopes: vec!["config:write".to_string()],
                },
                ScopedControlApiToken {
                    token: "beta".to_string(),
                    scopes: vec!["skills:write".to_string()],
                },
            ],
            ..MutatingAuthPolicy::default()
        };
        let token = find_control_api_token("beta", &policy).expect("token must be found");
        assert_eq!(token.token, "beta");
        assert!(find_control_api_token("missing", &policy).is_none());
    }

    #[test]
    fn parse_bearer_token_accepts_case_insensitive_scheme() {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            "bEaReR      token-value".parse().expect("header value"),
        );
        assert_eq!(parse_bearer_token(&headers).as_deref(), Some("token-value"));
    }

    #[tokio::test]
    async fn auth_middleware_rejects_missing_org_header() {
        let router = build_auth_test_router(MutatingAuthPolicy {
            require_auth_for_mutating: true,
            allow_insecure_mutating_requests: false,
            require_org_header_for_mutating: true,
            control_api_tokens: vec![ScopedControlApiToken {
                token: "token-alpha".to_string(),
                scopes: vec!["config:write".to_string()],
            }],
            mutating_auth_exempt_prefixes: Vec::new(),
        });

        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/os/config/apply")
                    .header(AUTHORIZATION, "Bearer token-alpha")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body = response_body_json(response).await;
        assert_eq!(
            body.get("error_code").and_then(|v| v.as_str()),
            Some("missing_or_invalid_org_id")
        );
    }

    #[tokio::test]
    async fn auth_middleware_rejects_missing_bearer_token() {
        let router = build_auth_test_router(MutatingAuthPolicy {
            require_auth_for_mutating: true,
            allow_insecure_mutating_requests: false,
            require_org_header_for_mutating: true,
            control_api_tokens: vec![ScopedControlApiToken {
                token: "token-alpha".to_string(),
                scopes: vec!["config:write".to_string()],
            }],
            mutating_auth_exempt_prefixes: Vec::new(),
        });
        let org_id = Uuid::new_v4();

        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/os/config/apply")
                    .header("x-org-id", org_id.to_string())
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body = response_body_json(response).await;
        assert_eq!(
            body.get("error_code").and_then(|v| v.as_str()),
            Some("missing_bearer_token")
        );
        assert_eq!(
            body.get("required_scope").and_then(|v| v.as_str()),
            Some("config:write")
        );
    }

    #[tokio::test]
    async fn auth_middleware_rejects_token_without_required_scope() {
        let router = build_auth_test_router(MutatingAuthPolicy {
            require_auth_for_mutating: true,
            allow_insecure_mutating_requests: false,
            require_org_header_for_mutating: true,
            control_api_tokens: vec![ScopedControlApiToken {
                token: "token-alpha".to_string(),
                scopes: vec!["messages:write".to_string()],
            }],
            mutating_auth_exempt_prefixes: Vec::new(),
        });
        let org_id = Uuid::new_v4();

        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/os/config/apply")
                    .header("x-org-id", org_id.to_string())
                    .header(AUTHORIZATION, "Bearer token-alpha")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body = response_body_json(response).await;
        assert_eq!(
            body.get("error_code").and_then(|v| v.as_str()),
            Some("missing_required_scope")
        );
        assert_eq!(
            body.get("required_scope").and_then(|v| v.as_str()),
            Some("config:write")
        );
    }

    #[tokio::test]
    async fn auth_middleware_allows_exempt_mutating_path() {
        let router = build_auth_test_router(MutatingAuthPolicy {
            require_auth_for_mutating: true,
            allow_insecure_mutating_requests: false,
            require_org_header_for_mutating: true,
            control_api_tokens: vec![ScopedControlApiToken {
                token: "token-alpha".to_string(),
                scopes: vec!["config:write".to_string()],
            }],
            mutating_auth_exempt_prefixes: vec!["/api/v1/os/automation/webhook/".to_string()],
        });

        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/os/automation/webhook/github")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body_json(response).await;
        assert_eq!(body.get("status").and_then(|v| v.as_str()), Some("ok"));
    }

    #[tokio::test]
    async fn unauthorized_keeps_back_compat_error_shape() {
        let response = unauthorized("missing bearer token");
        let response = response_body_json(response).await;
        assert_eq!(
            response.get("status").and_then(|v| v.as_str()),
            Some("error")
        );
        assert_eq!(
            response.get("error").and_then(|v| v.as_str()),
            Some("missing bearer token")
        );
    }
}
