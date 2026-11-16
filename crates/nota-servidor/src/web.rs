//! As rotas `/v1/...` e a autenticação.
//!
//! O log tem só aparelho, método, rota (o modelo, sem ids nem hashes) e status.
//! Nunca corpo, token ou conteúdo.

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, MatchedPath, Path, Query, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use nota_sync::protocolo::{BLOB_MAXIMO, CABECALHO_HASH, PedidoPublicar, hex64};
use serde::Deserialize;

use crate::banco::{Banco, ErroBlob, ErroPublicar, ResultadoPublicar};
use crate::tokens;

pub struct Estado {
    pub banco: Banco,
    pub dados: PathBuf,
}

type S = Arc<Estado>;

fn erro(status: StatusCode, msg: &'static str) -> Response {
    (status, msg).into_response()
}

fn interno<E: std::fmt::Display>(e: E) -> Response {
    // O texto do erro fica só no stderr do servidor, sem conteúdo de nota (o servidor não tem).
    eprintln!("erro interno: {e}");
    erro(StatusCode::INTERNAL_SERVER_ERROR, "erro interno")
}

pub fn roteador(estado: S) -> Router {
    Router::new()
        .route("/v1/mudancas", get(mudancas))
        .route("/v1/blob/{hash}", get(baixar_blob))
        .route(
            "/v1/blob",
            put(enviar_blob).layer(DefaultBodyLimit::max(BLOB_MAXIMO)),
        )
        .route("/v1/item/{id}", post(publicar))
        .route_layer(middleware::from_fn_with_state(estado.clone(), autenticar))
        .with_state(estado)
}

/// `Authorization: Bearer <token>` contra os hashes em `dados/tokens`, em tempo constante.
async fn autenticar(State(s): State<S>, req: Request, next: Next) -> Response {
    let rota = req
        .extensions()
        .get::<MatchedPath>()
        .map_or_else(|| "?".to_string(), |m| m.as_str().to_string());
    let metodo = req.method().clone();
    let apresentado = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .map(str::to_owned);
    // Relê a cada pedido: um token criado com o servidor no ar vale na hora.
    let entradas = tokens::carregar(&s.dados);
    let aparelho = apresentado.and_then(|t| tokens::conferir(&entradas, &t));
    let Some(aparelho) = aparelho else {
        eprintln!("(sem identidade) {metodo} {rota} -> 401");
        let mut r = erro(StatusCode::UNAUTHORIZED, "não autorizado");
        r.headers_mut().insert(
            header::WWW_AUTHENTICATE,
            "Bearer".parse().expect("valor fixo"),
        );
        return r;
    };
    let resp = next.run(req).await;
    eprintln!("{aparelho} {metodo} {rota} -> {}", resp.status().as_u16());
    resp
}

#[derive(Deserialize)]
struct ConsultaMudancas {
    #[serde(default)]
    desde: u64,
}

async fn mudancas(State(s): State<S>, Query(q): Query<ConsultaMudancas>) -> Response {
    let r = tokio::task::spawn_blocking(move || s.banco.mudancas(q.desde)).await;
    match r {
        Ok(Ok(m)) => Json(m).into_response(),
        Ok(Err(e)) => interno(e),
        Err(e) => interno(e),
    }
}

async fn baixar_blob(State(s): State<S>, Path(hash): Path<String>) -> Response {
    if !hex64(&hash) {
        return erro(StatusCode::BAD_REQUEST, "hash inválido");
    }
    let r = tokio::task::spawn_blocking(move || s.banco.ler_blob(&hash)).await;
    match r {
        Ok(Ok(Some(b))) => (
            [(header::CONTENT_TYPE, "application/octet-stream")],
            Bytes::from(b),
        )
            .into_response(),
        Ok(Ok(None)) => erro(StatusCode::NOT_FOUND, "blob não encontrado"),
        Ok(Err(e)) => interno(e),
        Err(e) => interno(e),
    }
}

async fn enviar_blob(State(s): State<S>, headers: HeaderMap, corpo: Bytes) -> Response {
    let esperado = match headers.get(CABECALHO_HASH) {
        None => None,
        Some(v) => match v.to_str() {
            Ok(h) if hex64(h) => Some(h.to_string()),
            _ => return erro(StatusCode::BAD_REQUEST, "hash informado inválido"),
        },
    };
    let r = tokio::task::spawn_blocking(move || s.banco.guardar_blob(&corpo, esperado.as_deref()))
        .await;
    match r {
        Ok(Ok(Ok(hash))) => Json(serde_json::json!({ "hash": hash })).into_response(),
        Ok(Ok(Err(ErroBlob::Vazio))) => erro(StatusCode::BAD_REQUEST, "blob vazio"),
        Ok(Ok(Err(ErroBlob::GrandeDemais))) => {
            erro(StatusCode::PAYLOAD_TOO_LARGE, "blob grande demais")
        }
        Ok(Ok(Err(ErroBlob::HashNaoBate))) => erro(
            StatusCode::BAD_REQUEST,
            "o SHA-256 do corpo não bate com o informado",
        ),
        Ok(Err(e)) => interno(e),
        Err(e) => interno(e),
    }
}

async fn publicar(
    State(s): State<S>,
    Path(id): Path<String>,
    Json(pedido): Json<PedidoPublicar>,
) -> Response {
    let r = tokio::task::spawn_blocking(move || s.banco.publicar(&id, &pedido)).await;
    match r {
        Ok(Ok(Ok(ResultadoPublicar::Ok(r)))) => Json(r).into_response(),
        Ok(Ok(Ok(ResultadoPublicar::Conflito(c)))) => {
            (StatusCode::CONFLICT, Json(c)).into_response()
        }
        Ok(Ok(Err(ErroPublicar::IdInvalido))) => erro(StatusCode::BAD_REQUEST, "id inválido"),
        Ok(Ok(Err(ErroPublicar::HashInvalido))) => erro(StatusCode::BAD_REQUEST, "hash inválido"),
        Ok(Ok(Err(ErroPublicar::BlobDesconhecido))) => {
            erro(StatusCode::BAD_REQUEST, "envie o blob antes de publicar")
        }
        Ok(Err(e)) => interno(e),
        Err(e) => interno(e),
    }
}
