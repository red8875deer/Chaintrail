//! `chaintrail serve`: the app's interface over HTTP on this computer only.
//!
//! Used for browser-based development and testing of the UI, and by anyone who prefers a
//! browser to the desktop app. It listens on 127.0.0.1 only and every request must carry
//! a random token printed at startup, so other websites cannot use it.
//!
//! API: `POST /api/<command>` with the command's request as JSON; the same commands and
//! shapes as the desktop app (`ct_service::views`).

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use ct_service::{AppService, ErrorKind, ServiceError};
use serde::de::DeserializeOwned;
use serde_json::Value;
use tower_http::services::{ServeDir, ServeFile};

pub const TOKEN_HEADER: &str = "x-chaintrail-token";
/// Names a running trace so the page can ask for its progress or stop it.
pub const TRACE_ID_HEADER: &str = "x-chaintrail-trace-id";

/// A trace in progress: its latest progress, and the flag that stops it.
#[derive(Default)]
struct TraceRun {
    stop: AtomicBool,
    progress: Mutex<Option<ct_trace::Progress>>,
}

struct AppState {
    service: AppService,
    token: String,
    traces: Mutex<HashMap<String, Arc<TraceRun>>>,
}

#[derive(serde::Deserialize)]
struct TraceIdRequest {
    id: String,
}

pub fn new_token() -> Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("random token: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

pub fn router(service: AppService, token: String, ui_dir: Option<PathBuf>) -> Router {
    let state = Arc::new(AppState {
        service,
        token,
        traces: Mutex::new(HashMap::new()),
    });
    let api = Router::new()
        .route("/api/{command}", post(api))
        .with_state(state);
    let app = match ui_dir {
        Some(dir) => {
            let index = dir.join("index.html");
            api.fallback_service(ServeDir::new(dir).fallback(ServeFile::new(index)))
        }
        None => api,
    };
    app.layer(axum::middleware::map_response(security_headers))
}

/// The page may load only its own files, never be framed by another site, and its files are
/// never reinterpreted as another type.
async fn security_headers(mut res: Response) -> Response {
    let h = res.headers_mut();
    h.insert(
        "content-security-policy",
        axum::http::HeaderValue::from_static(
            "default-src 'self'; img-src 'self' data: blob:; style-src 'self' 'unsafe-inline'; \
             object-src 'none'; base-uri 'none'; frame-ancestors 'none'",
        ),
    );
    h.insert(
        "x-frame-options",
        axum::http::HeaderValue::from_static("DENY"),
    );
    h.insert(
        "x-content-type-options",
        axum::http::HeaderValue::from_static("nosniff"),
    );
    h.insert(
        "referrer-policy",
        axum::http::HeaderValue::from_static("no-referrer"),
    );
    res
}

/// Only this computer's own names: a page on another site that rebinds its domain to
/// 127.0.0.1 sends its own name as Host and is refused (the token is a second line).
fn local_host(headers: &HeaderMap) -> bool {
    headers
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(|h| h.rsplit_once(':').map_or(h, |(name, _)| name))
        .is_some_and(|name| matches!(name, "127.0.0.1" | "localhost" | "[::1]"))
}

pub async fn run(
    service: AppService,
    port: u16,
    ui_dir: Option<PathBuf>,
    token: String,
) -> Result<()> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("listening on {addr}"))?;
    let has_ui = ui_dir.is_some();
    let app = router(service, token.clone(), ui_dir);
    if has_ui {
        eprintln!("ChainTrail is running. Open: http://{addr}/#token={token}");
    } else {
        eprintln!("ChainTrail API on http://{addr}/api/<command> (header {TOKEN_HEADER}: {token})");
        eprintln!("No UI folder found; build it with `npm run build` in app/ui or pass --ui DIR.");
    }
    eprintln!("Only this computer can connect. Press Ctrl+C to stop.");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

fn parse<T: DeserializeOwned>(body: &Bytes) -> Result<T, ServiceError> {
    let body: &[u8] = if body.is_empty() { b"{}" } else { body };
    serde_json::from_slice(body).map_err(|e| ServiceError::invalid(format!("bad request: {e}")))
}

fn json<T: serde::Serialize>(v: T) -> Result<Value, ServiceError> {
    serde_json::to_value(v).map_err(|e| ServiceError::internal(e.to_string()))
}

async fn api(
    State(state): State<Arc<AppState>>,
    Path(command): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let authorised = headers
        .get(TOKEN_HEADER)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|t| constant_time_eq(t.as_bytes(), state.token.as_bytes()));
    if !authorised {
        return (StatusCode::UNAUTHORIZED, "missing or wrong token").into_response();
    }
    if !local_host(&headers) {
        return (
            StatusCode::FORBIDDEN,
            "only this computer may use ChainTrail",
        )
            .into_response();
    }
    let s = &state.service;
    let result = match command.as_str() {
        "status" => json(s.status()),
        "resolve" => match parse(&body) {
            Ok(r) => s.resolve(r).and_then(json),
            Err(e) => Err(e),
        },
        "btc_address" => match parse(&body) {
            Ok(r) => s.btc_address(r).await.and_then(json),
            Err(e) => Err(e),
        },
        "btc_tx" => match parse(&body) {
            Ok(r) => s.btc_tx(r).await.and_then(json),
            Err(e) => Err(e),
        },
        "btc_trace" => match parse(&body) {
            Ok(r) => {
                // A short id from the page (letters, digits, dashes) to follow this trace.
                let id = headers
                    .get(TRACE_ID_HEADER)
                    .and_then(|v| v.to_str().ok())
                    .filter(|v| {
                        !v.is_empty()
                            && v.len() <= 64
                            && v.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                    })
                    .map(str::to_owned);
                let run = Arc::new(TraceRun::default());
                if let Some(id) = &id {
                    state.traces.lock().unwrap().insert(id.clone(), run.clone());
                }
                let progress_run = run.clone();
                let result = s
                    .btc_trace_stoppable(
                        r,
                        &mut move |p| *progress_run.progress.lock().unwrap() = Some(p),
                        &run.stop,
                    )
                    .await
                    .and_then(json);
                if let Some(id) = &id {
                    state.traces.lock().unwrap().remove(id);
                }
                result
            }
            Err(e) => Err(e),
        },
        // Progress of a running trace (null when it is not running).
        "trace_progress" => match parse::<TraceIdRequest>(&body) {
            Ok(r) => {
                let run = state.traces.lock().unwrap().get(&r.id).cloned();
                Ok(match run.and_then(|run| *run.progress.lock().unwrap()) {
                    Some(p) => serde_json::json!({
                        "txs_examined": p.txs_examined,
                        "pending": p.pending,
                        "leads": p.leads,
                    }),
                    None => Value::Null,
                })
            }
            Err(e) => Err(e),
        },
        // Stop a running trace: it ends early with what it found.
        "trace_stop" => match parse::<TraceIdRequest>(&body) {
            Ok(r) => {
                if let Some(run) = state.traces.lock().unwrap().get(&r.id) {
                    run.stop.store(true, Ordering::Relaxed);
                }
                Ok(serde_json::json!({}))
            }
            Err(e) => Err(e),
        },
        "label_lookup" => match parse(&body) {
            Ok(r) => s.label_lookup(r).and_then(json),
            Err(e) => Err(e),
        },
        "label_packs" => json(s.label_packs()),
        "label_add" => match parse(&body) {
            Ok(r) => s.label_add(r).and_then(json),
            Err(e) => Err(e),
        },
        "update_ofac" => s.update_ofac().await.and_then(json),
        other => Err(ServiceError::new(
            ErrorKind::NotFound,
            format!("unknown command: {other}"),
        )),
    };
    match result {
        Ok(v) => axum::Json(v).into_response(),
        Err(e) => {
            let status = match e.kind {
                ErrorKind::InvalidInput => StatusCode::BAD_REQUEST,
                ErrorKind::NotFound => StatusCode::NOT_FOUND,
                ErrorKind::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
                // 499: the client stopped the request (nginx's convention).
                ErrorKind::Cancelled => {
                    StatusCode::from_u16(499).unwrap_or(StatusCode::BAD_REQUEST)
                }
                ErrorKind::Internal => StatusCode::INTERNAL_SERVER_ERROR,
            };
            (status, axum::Json(e)).into_response()
        }
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use ct_service::ServiceConfig;
    use ct_trace::testing::{MemoryBitcoin, addr, tx, txid};

    async fn start() -> (String, String) {
        let t1 = tx(
            &txid(1),
            100,
            &[(&txid(900), 0, &addr(1), 100_010_000)],
            &[(&addr(2), 100_000_000)],
        );
        let service = AppService::with_provider(
            Box::new(MemoryBitcoin::new([t1])),
            ServiceConfig::default(),
            None,
        )
        .unwrap();
        let token = new_token().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = router(service, token.clone(), None);
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("http://{addr}"), token)
    }

    async fn post(base: &str, cmd: &str, token: Option<&str>, body: &str) -> (u16, String) {
        // Minimal HTTP/1.1 client over a raw socket, to avoid a test-only HTTP dependency.
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let host = base.trim_start_matches("http://");
        let mut s = tokio::net::TcpStream::connect(host).await.unwrap();
        let auth = token
            .map(|t| format!("{TOKEN_HEADER}: {t}\r\n"))
            .unwrap_or_default();
        let req = format!(
            "POST /api/{cmd} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\n{auth}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        s.write_all(req.as_bytes()).await.unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).await.unwrap();
        let status: u16 = out[9..12].parse().unwrap();
        let body = out.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
        (status, body)
    }

    #[tokio::test]
    async fn requires_token() {
        let (base, _) = start().await;
        assert_eq!(post(&base, "status", None, "").await.0, 401);
        assert_eq!(post(&base, "status", Some("wrong"), "").await.0, 401);
    }

    #[tokio::test]
    async fn commands_and_errors() {
        let (base, token) = start().await;
        let (code, body) = post(&base, "status", Some(&token), "").await;
        assert_eq!(code, 200, "{body}");
        assert!(body.contains("labelled_addresses"));

        let (code, body) = post(
            &base,
            "btc_tx",
            Some(&token),
            &format!(r#"{{"txid":"{}"}}"#, txid(1)),
        )
        .await;
        assert_eq!(code, 200, "{body}");
        assert!(body.contains("\"outputs\""));

        let (code, body) = post(&base, "resolve", Some(&token), r#"{"query":"nonsense"}"#).await;
        assert_eq!(code, 400);
        assert!(body.contains("invalid_input"));

        let (code, _) = post(&base, "nope", Some(&token), "").await;
        assert_eq!(code, 404);
    }
}
