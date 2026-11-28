//! ChainTrail desktop app: a Tauri window around the UI in `app/ui`.
//!
//! Every command maps one-to-one onto `ct_service::AppService`, with the same names and
//! request/response shapes as `chaintrail serve`. Commands take one argument named `req`.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use ct_service::views::*;
use ct_service::{AppService, ServiceConfig, ServiceError};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};

/// Running traces by the id the page gave them, with the flag that stops each.
#[derive(Default)]
struct TraceRuns(Mutex<HashMap<String, Arc<AtomicBool>>>);

/// Request naming a running trace.
#[derive(Debug, Deserialize)]
struct TraceIdRequest {
    id: String,
}

/// Request type for commands without parameters.
#[derive(Debug, Default, Deserialize)]
struct Empty {}

/// Payload of the `trace_progress` event.
#[derive(Debug, Clone, Serialize)]
struct TraceProgress {
    /// The trace this belongs to (a page may have started a newer one).
    id: Option<String>,
    txs_examined: u32,
    pending: usize,
    leads: usize,
}

type Res<T> = Result<T, ServiceError>;

#[tauri::command]
fn status(service: State<'_, AppService>, req: Empty) -> StatusView {
    let _ = req;
    service.status()
}

#[tauri::command]
fn resolve(service: State<'_, AppService>, req: ResolveRequest) -> Res<ResolveView> {
    service.resolve(req)
}

#[tauri::command]
async fn btc_address(service: State<'_, AppService>, req: AddressRequest) -> Res<AddressView> {
    service.btc_address(req).await
}

#[tauri::command]
async fn btc_tx(service: State<'_, AppService>, req: TxRequest) -> Res<TxView> {
    service.btc_tx(req).await
}

#[tauri::command]
async fn btc_trace(
    app: tauri::AppHandle,
    service: State<'_, AppService>,
    runs: State<'_, TraceRuns>,
    req: TraceRequest,
    id: Option<String>,
) -> Res<TraceView> {
    let stop = Arc::new(AtomicBool::new(false));
    if let Some(id) = &id {
        runs.0.lock().unwrap().insert(id.clone(), stop.clone());
    }
    let mut last = std::time::Instant::now() - std::time::Duration::from_secs(1);
    let progress_id = id.clone();
    let result = service
        .btc_trace_stoppable(
            req,
            &mut |p: ct_trace::Progress| {
                // At most ~10 events per second.
                if last.elapsed() >= std::time::Duration::from_millis(100) {
                    last = std::time::Instant::now();
                    let _ = app.emit(
                        "trace_progress",
                        TraceProgress {
                            id: progress_id.clone(),
                            txs_examined: p.txs_examined,
                            pending: p.pending,
                            leads: p.leads,
                        },
                    );
                }
            },
            &stop,
        )
        .await;
    if let Some(id) = &id {
        runs.0.lock().unwrap().remove(id);
    }
    result
}

/// Stop a running trace: it ends early with what it found.
#[tauri::command]
fn trace_stop(runs: State<'_, TraceRuns>, req: TraceIdRequest) {
    if let Some(stop) = runs.0.lock().unwrap().get(&req.id) {
        stop.store(true, Ordering::Relaxed);
    }
}

#[tauri::command]
fn label_lookup(service: State<'_, AppService>, req: LabelLookupRequest) -> Res<Vec<LabelView>> {
    service.label_lookup(req)
}

#[tauri::command]
fn label_packs(service: State<'_, AppService>, req: Empty) -> Vec<PackView> {
    let _ = req;
    service.label_packs()
}

#[tauri::command]
fn label_add(service: State<'_, AppService>, req: AddLabelRequest) -> Res<Vec<LabelView>> {
    service.label_add(req)
}

#[tauri::command]
async fn update_ofac(service: State<'_, AppService>, req: Empty) -> Res<OfacUpdateView> {
    let _ = req;
    service.update_ofac().await
}

/// Warnings and errors go to `chaintrail.log` in the app's data folder (the window has no
/// console), so problems such as a data source falling back can be looked up later.
fn start_log() -> Option<std::path::PathBuf> {
    let dir = ct_store::app_data_dir()?;
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join("chaintrail.log");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok()?;
    tracing_subscriber::fmt()
        .with_max_level(tracing_subscriber::filter::LevelFilter::WARN)
        .with_writer(std::sync::Mutex::new(file))
        .with_ansi(false)
        .with_target(false)
        .init();
    Some(path)
}

fn main() {
    let log = start_log();
    let service = match AppService::new(ServiceConfig::default()) {
        Ok(s) => s,
        Err(e) => {
            // Rare: a damaged cache or label file is worked around inside the service.
            tracing::error!("ChainTrail could not start: {e}");
            eprintln!(
                "ChainTrail could not start: {e}{}",
                log.map(|p| format!(" (details in {})", p.display()))
                    .unwrap_or_default()
            );
            std::process::exit(1);
        }
    };
    tauri::Builder::default()
        .manage(service)
        .manage(TraceRuns::default())
        .invoke_handler(tauri::generate_handler![
            trace_stop,
            status,
            resolve,
            btc_address,
            btc_tx,
            btc_trace,
            label_lookup,
            label_packs,
            label_add,
            update_ofac
        ])
        .run(tauri::generate_context!())
        .expect("error while running ChainTrail");
}
