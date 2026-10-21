use std::{error::Error, io};

use axum::{
    Json,
    extract::{Query, Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
};
use openidconnect::{
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointMaybeSet, EndpointNotSet,
    EndpointSet, IssuerUrl, Nonce, OAuth2TokenResponse, PkceCodeChallenge, PkceCodeVerifier,
    RedirectUrl, Scope, TokenResponse,
    core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata},
    reqwest,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha512};
use tower_sessions::{
    MemoryStore, Session, SessionManagerLayer,
    cookie::{Key, SameSite},
};

type OidcClient = CoreClient<
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointMaybeSet,
    EndpointMaybeSet,
>;

#[derive(Clone)]
pub struct AuthState {
    client: OidcClient,
    http: reqwest::Client,
    frontend_url: String,
    logout_url: String,
    required_claim: String,
    required_role: String,
}

impl AuthState {
    pub async fn discover(
        discovery_url: &str,
        client_id: String,
        client_secret: String,
        redirect_uri: &str,
        frontend_url: &str,
        logout_url: &str,
        required_claim: String,
        required_role: String,
    ) -> Result<Self, Box<dyn Error>> {
        if required_claim.trim().is_empty() || required_role.trim().is_empty() {
            return Err(io::Error::other(
                "OAUTH_REQUIRED_CLAIM and OAUTH_REQUIRED_ROLE must not be empty",
            )
            .into());
        }
        let http = reqwest::ClientBuilder::new()
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        // Read the configured document to obtain its issuer, then let the OIDC
        // library discover and validate the provider and its signing keys.
        let response = http.get(discovery_url).send().await?.error_for_status()?;
        let document: serde_json::Value = serde_json::from_slice(&response.bytes().await?)?;
        let issuer = document["issuer"]
            .as_str()
            .ok_or_else(|| io::Error::other("OIDC discovery document has no issuer"))?;
        let metadata =
            CoreProviderMetadata::discover_async(IssuerUrl::new(issuer.to_owned())?, &http).await?;
        let client = CoreClient::from_provider_metadata(
            metadata,
            ClientId::new(client_id),
            Some(ClientSecret::new(client_secret)),
        )
        .set_redirect_uri(RedirectUrl::new(redirect_uri.to_owned())?);

        Ok(Self {
            client,
            http,
            frontend_url: frontend_url.to_owned(),
            logout_url: logout_url.to_owned(),
            required_claim,
            required_role,
        })
    }
}

pub fn session_layer(
    secret: &str,
    redirect_uri: &str,
) -> Result<SessionManagerLayer<MemoryStore, tower_sessions::service::SignedCookie>, Box<dyn Error>>
{
    let redirect = reqwest::Url::parse(redirect_uri)?;
    if secret.is_empty() {
        return Err(io::Error::other("SESSION_SECRET must not be empty").into());
    }
    let key = Key::from(&Sha512::digest(secret.as_bytes()));
    Ok(SessionManagerLayer::new(MemoryStore::default())
        .with_name("scepa_session")
        .with_path("/upload/api")
        .with_same_site(SameSite::Lax)
        .with_secure(redirect.scheme() == "https")
        .with_signed(key))
}

#[derive(Serialize, Deserialize)]
struct PendingLogin {
    state: String,
    nonce: String,
    pkce_verifier: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct User {
    sub: String,
    email: Option<String>,
    name: Option<String>,
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    code: String,
    state: String,
}

pub async fn login(
    State(auth): State<AuthState>,
    session: Session,
) -> Result<Redirect, StatusCode> {
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    let (url, state, nonce) = auth
        .client
        .authorize_url(
            CoreAuthenticationFlow::AuthorizationCode,
            CsrfToken::new_random,
            Nonce::new_random,
        )
        .add_scope(Scope::new("email".to_owned()))
        .add_scope(Scope::new("profile".to_owned()))
        .set_pkce_challenge(challenge)
        .url();
    session
        .insert(
            "pending_login",
            PendingLogin {
                state: state.secret().to_owned(),
                nonce: nonce.secret().to_owned(),
                pkce_verifier: verifier.secret().to_owned(),
            },
        )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Redirect::temporary(url.as_str()))
}

pub async fn callback(
    State(auth): State<AuthState>,
    session: Session,
    Query(query): Query<CallbackQuery>,
) -> Result<Redirect, StatusCode> {
    let pending = session
        .remove::<PendingLogin>("pending_login")
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::BAD_REQUEST)?;
    if pending.state != query.state {
        return Err(StatusCode::BAD_REQUEST);
    }
    let token = auth
        .client
        .exchange_code(AuthorizationCode::new(query.code))
        .map_err(|_| StatusCode::BAD_GATEWAY)?
        .set_pkce_verifier(PkceCodeVerifier::new(pending.pkce_verifier))
        .request_async(&auth.http)
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    let id_token = token.id_token().ok_or(StatusCode::BAD_GATEWAY)?;
    let claims = id_token
        .claims(&auth.client.id_token_verifier(), &Nonce::new(pending.nonce))
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    let user_info_url = auth.client.user_info_url().ok_or(StatusCode::BAD_GATEWAY)?;
    let user_info_response = auth
        .http
        .get(user_info_url.as_str())
        .bearer_auth(token.access_token().secret())
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?
        .error_for_status()
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    let user_info: serde_json::Value = serde_json::from_slice(
        &user_info_response
            .bytes()
            .await
            .map_err(|_| StatusCode::BAD_GATEWAY)?,
    )
    .map_err(|_| StatusCode::BAD_GATEWAY)?;
    if let Some(reason) = role_denial_reason(
        &user_info,
        claims.subject().as_str(),
        &auth.required_claim,
        &auth.required_role,
    ) {
        tracing::warn!(reason, "OIDC role check rejected login");
        session
            .flush()
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        return Err(StatusCode::FORBIDDEN);
    }
    session
        .cycle_id()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    session
        .insert(
            "user",
            User {
                sub: claims.subject().as_str().to_owned(),
                email: claims.email().map(|email| email.as_str().to_owned()),
                name: claims
                    .name()
                    .and_then(|name| name.get(None))
                    .map(|name| name.as_str().to_owned()),
            },
        )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Redirect::to(&auth.frontend_url))
}

fn role_denial_reason(
    user_info: &serde_json::Value,
    subject: &str,
    claim: &str,
    role: &str,
) -> Option<&'static str> {
    if user_info["sub"].as_str() != Some(subject) {
        return Some("subject_mismatch");
    }
    let Some(values) = user_info[claim].as_array() else {
        return Some("claim_missing_or_not_array");
    };
    if !values.iter().any(|value| value.as_str() == Some(role)) {
        return Some("required_role_absent");
    }
    None
}

pub async fn logout(
    State(auth): State<AuthState>,
    session: Session,
) -> Result<Redirect, StatusCode> {
    logout_to(session, &auth.logout_url).await
}

async fn logout_to(session: Session, logout_url: &str) -> Result<Redirect, StatusCode> {
    session
        .flush()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Redirect::to(logout_url))
}

pub async fn me(session: Session) -> Result<Json<User>, StatusCode> {
    session
        .get::<User>("user")
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map(Json)
        .ok_or(StatusCode::UNAUTHORIZED)
}

pub async fn require_user(session: Session, request: Request, next: Next) -> Response {
    match session.get::<User>("user").await {
        Ok(Some(_)) => next.run(request).await,
        Ok(None) => StatusCode::UNAUTHORIZED.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        body::Body,
        http::{Request, header},
        middleware,
        routing::get,
    };
    use tower::ServiceExt;

    #[test]
    fn required_role_must_match_the_verified_user_and_exact_claim_value() {
        let info = serde_json::json!({"sub": "operator", "roles": ["default", "SCEPA:EDITOR"]});
        assert_eq!(
            role_denial_reason(&info, "operator", "roles", "SCEPA:EDITOR"),
            None
        );
        assert_eq!(
            role_denial_reason(&info, "other", "roles", "SCEPA:EDITOR"),
            Some("subject_mismatch")
        );
        assert_eq!(
            role_denial_reason(&info, "operator", "roles", "SCEPA"),
            Some("required_role_absent")
        );
        assert_eq!(
            role_denial_reason(&info, "operator", "groups", "SCEPA:EDITOR"),
            Some("claim_missing_or_not_array")
        );
        assert_eq!(
            role_denial_reason(
                &serde_json::json!({"sub": "operator"}),
                "operator",
                "roles",
                "SCEPA:EDITOR"
            ),
            Some("claim_missing_or_not_array")
        );
    }

    #[tokio::test]
    async fn document_routes_require_a_valid_session_and_logout_revokes_it() {
        let protected = Router::new()
            .route("/documents", get(|| async { StatusCode::OK }))
            .route_layer(middleware::from_fn(require_user));
        let app = Router::new()
            .route(
                "/auth/logout",
                get(|session: Session| async move {
                    logout_to(
                        session,
                        "https://auth.example/application/o/kg-dev/end-session/",
                    )
                    .await
                }),
            )
            .route(
                "/test-login",
                get(|session: Session| async move {
                    session
                        .insert(
                            "user",
                            User {
                                sub: "operator".to_owned(),
                                email: None,
                                name: None,
                            },
                        )
                        .await
                        .unwrap();
                    StatusCode::OK
                }),
            )
            .merge(protected)
            .layer(
                session_layer(
                    "independent-scepa-secret",
                    "http://localhost:5173/upload/api/auth/callback",
                )
                .unwrap(),
            );

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/documents")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let login = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/test-login")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let cookie = login
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap();
        let authenticated = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/documents")
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(authenticated.status(), StatusCode::OK);

        let logged_out = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/auth/logout")
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(logged_out.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            logged_out.headers().get(header::LOCATION).unwrap(),
            "https://auth.example/application/o/kg-dev/end-session/"
        );

        let revoked = app
            .oneshot(
                Request::builder()
                    .uri("/documents")
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(revoked.status(), StatusCode::UNAUTHORIZED);
    }
}
