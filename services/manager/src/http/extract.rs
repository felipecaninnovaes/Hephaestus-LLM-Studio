//! Extractors tipados axum para UUIDs e JSON com semântica de erro preservada (MM-03, MM-04).

use async_trait::async_trait;
use axum::{
    body::Bytes,
    extract::{FromRequest, FromRequestParts, Path, Request},
    http::request::Parts,
    http::StatusCode,
    response::Response,
};
use serde::de::DeserializeOwned;
use uuid::Uuid;

use crate::error::{bad_request, error_response, not_found};

macro_rules! impl_path_uuid {
    ($name:ident, $is_not_found:expr) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct $name(pub Uuid);

        #[async_trait]
        impl<S> FromRequestParts<S> for $name
        where
            S: Send + Sync,
        {
            type Rejection = Response;

            async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
                let Path(id_str) = Path::<String>::from_request_parts(parts, state)
                    .await
                    .map_err(|_| if $is_not_found { not_found() } else { bad_request("invalid uuid") })?;
                let u = id_str.parse::<Uuid>()
                    .map_err(|_| if $is_not_found { not_found() } else { bad_request("invalid uuid") })?;
                Ok(Self(u))
            }
        }
    };
}

impl_path_uuid!(PathUuidNotFound, true);
impl_path_uuid!(PathUuidBadRequest, false);
impl_path_uuid!(JobId, true);
impl_path_uuid!(OrchestratorId, true);
impl_path_uuid!(ModelId, false);
impl_path_uuid!(GenerationId, false);

/// Extrator JSON com validação de corpo vazio e mensagem de erro padronizada (MM-04).
/// Corpo vazio -> 400 invalid_request: empty body.
/// JSON malformado -> 400 invalid_request: invalid json: {e}.
#[derive(Debug, Clone)]
pub struct AppJson<T>(pub T);

#[async_trait]
impl<S, T> FromRequest<S> for AppJson<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let bytes = Bytes::from_request(req, state)
            .await
            .map_err(|e| bad_request(&e.to_string()))?;
        if bytes.is_empty() {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "empty body",
            ));
        }
        let val = serde_json::from_slice(&bytes).map_err(|e| {
            error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                &format!("invalid json: {e}"),
            )
        })?;
        Ok(Self(val))
    }
}

/// Extrator JSON opcional para endpoints onde corpo vazio é válido (ex: cleanup_jobs).
#[derive(Debug, Clone)]
pub struct OptionalAppJson<T>(pub Option<T>);

#[async_trait]
impl<S, T> FromRequest<S> for OptionalAppJson<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let bytes = Bytes::from_request(req, state)
            .await
            .map_err(|e| bad_request(&e.to_string()))?;
        if bytes.is_empty() {
            return Ok(Self(None));
        }
        let val = serde_json::from_slice(&bytes).map_err(|e| {
            error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                &format!("invalid json: {e}"),
            )
        })?;
        Ok(Self(Some(val)))
    }
}
