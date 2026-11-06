//! Who calls, and what they may do.
//!
//! The console's server side sends the signed-in person's access token (`aud` pacod-aggregator) as
//! a bearer token; a consumer such as a gateway sends a service token of pacod-id. Tokens are
//! checked against pacod-id, and the roles come from the copy of the bindings its feed keeps
//! current. With `AGG_AUTH=dev` one static token stands for an admin instead, for development
//! without pacod-id.

use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::StatusCode;
use axum::http::request::Parts;
use pacod_id_authz::{Binding, ScopeRef, Subject};
use pacod_id_client::{AuthError, Caller};
use subtle::ConstantTimeEq;

use super::{ApiError, AppState};
use crate::identity::{CATALOG, Identity};

pub enum AuthMode {
    Oidc(Arc<Identity>),
    Dev { token: String },
}

/// The permissions of the manifest, in the order the console shows them.
pub const PERMISSIONS: [&str; 4] = ["agg.catalog.read", "agg.data.read", "agg.data.manage", "agg.members.manage"];

#[derive(Clone)]
pub struct Principal {
    pub id: String,
    /// Email or name, for the records of who decided what, and the console.
    pub display: String,
    pub kind: &'static str,
    subject: Subject,
}

impl Principal {
    pub fn new(id: String, display: String, kind: &'static str, bindings: Vec<Binding>) -> Principal {
        let subject = if kind == "service" { Subject::service(bindings) } else { Subject::user(bindings) };
        Principal { id, display, kind, subject }
    }

    pub fn allowed(&self, permission: &str) -> bool {
        CATALOG.allowed(&self.subject, permission, &[ScopeRef::platform()])
    }

    pub fn require(&self, permission: &str) -> Result<(), ApiError> {
        if self.allowed(permission) {
            Ok(())
        } else {
            Err(ApiError::new(StatusCode::FORBIDDEN, "forbidden", format!("this needs {permission}")))
        }
    }

    pub fn permissions(&self) -> Vec<&'static str> {
        PERMISSIONS.into_iter().filter(|p| self.allowed(p)).collect()
    }
}

/// The bearer token of a request.
pub fn bearer(parts: &Parts) -> Option<&str> {
    let value = parts.headers.get("authorization")?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    (scheme.eq_ignore_ascii_case("bearer") && !token.trim().is_empty()).then(|| token.trim())
}

fn unauthorized() -> ApiError {
    ApiError::new(StatusCode::UNAUTHORIZED, "unauthorized", "invalid or expired credentials")
}

async fn authenticate(auth: &AuthMode, token: &str) -> Result<Principal, ApiError> {
    match auth {
        AuthMode::Dev { token: dev } => {
            if !bool::from(token.as_bytes().ct_eq(dev.as_bytes())) {
                return Err(unauthorized());
            }
            let admin = vec![Binding::new("agg.admin", vec![ScopeRef::platform()])];
            Ok(Principal::new("dev".into(), "development".into(), "user", admin))
        }
        AuthMode::Oidc(id) => {
            if !id.fresh() {
                return Err(ApiError::unavailable("pacod-id's copy of the roles is not current; try again shortly"));
            }
            let caller = id.auth.authenticate(token).await.map_err(|e| match e {
                AuthError::Unavailable(why) => {
                    tracing::warn!(error = %why, "pacod-id could not check a token");
                    ApiError::unavailable("pacod-id cannot be reached; try again shortly")
                }
                _ => unauthorized(),
            })?;
            Ok(match caller {
                Caller::User { sub, email, name, bindings, .. } => {
                    let display = email.or(name).unwrap_or_else(|| sub.clone());
                    Principal::new(sub, display, "user", bindings)
                }
                Caller::Service { sub, name, bindings, .. } => Principal::new(sub, name, "service", bindings),
            })
        }
    }
}

impl FromRequestParts<AppState> for Principal {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let token = bearer(parts).ok_or_else(unauthorized)?;
        let p = authenticate(&state.auth, token).await?;
        // Everything but the health checks needs some aggregator role.
        p.require("agg.catalog.read")
            .map_err(|_| ApiError::new(StatusCode::FORBIDDEN, "no_access", "this account has no aggregator role"))?;
        Ok(p)
    }
}

/// A caller who may see what the console shows.
pub struct Reader(pub Principal);

impl FromRequestParts<AppState> for Reader {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let p = Principal::from_request_parts(parts, state).await?;
        p.require("agg.data.read")?;
        Ok(Reader(p))
    }
}

/// A caller who may change vendors, pages and settings, and start runs.
pub struct Manager(pub Principal);

impl FromRequestParts<AppState> for Manager {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let p = Principal::from_request_parts(parts, state).await?;
        p.require("agg.data.manage")?;
        Ok(Manager(p))
    }
}

/// A caller of the public catalogue: anyone with `AGG_CATALOG_PUBLIC`, else a holder of
/// `agg.catalog.read`.
pub struct CatalogReader;

impl FromRequestParts<AppState> for CatalogReader {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        if !state.config.catalog_public {
            Principal::from_request_parts(parts, state).await?;
        }
        Ok(CatalogReader)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_role_gets_its_permissions_and_nothing_else() {
        let p = |role: &str| {
            Principal::new("u".into(), "u".into(), "user", vec![Binding::new(role, vec![ScopeRef::platform()])])
        };
        assert_eq!(p("agg.consumer").permissions(), ["agg.catalog.read"]);
        assert_eq!(p("agg.viewer").permissions(), ["agg.catalog.read", "agg.data.read"]);
        assert_eq!(p("agg.admin").permissions().len(), 4);
        assert!(p("gw.platform_admin").permissions().is_empty(), "another product's role grants nothing here");
    }
}
