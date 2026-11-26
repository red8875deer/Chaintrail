//! ChainTrail desktop app: a Tauri window around the UI in `app/ui`.
//!
//! Every command maps one-to-one onto `ct_service::AppService`, with the same names and
//! request/response shapes as `chaintrail serve`. Commands take one argument named `req`.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use ct_service::views::*;
use ct_service::{AppService, ServiceConfig, ServiceError};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};

/// Request type for commands without parameters.
#[derive(Debug, Default, Deserialize)]
struct Empty {}

/// Payload of the `trace_progress` event.
#[derive(Debug, Clone, Serialize)]
struct TraceProgress {
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
    req: TraceRequest,
) -> Res<TraceView> {
    let mut last = std::time::Instant::now() - std::time::Duration::from_secs(1);
    service
        .btc_trace(req, &mut |p: ct_trace::Progress| {
            // At most ~10 events per second.
            if last.elapsed() >= std::time::Duration::from_millis(100) {
                last = std::time::Instant::now();
                let _ = app.emit(
                    "trace_progress",
                    TraceProgress {
                        txs_examined: p.txs_examined,
                        pending: p.pending,
                        leads: p.leads,
                    },
                );
            }
        })
        .await
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

fn main() {
    let service = match AppService::new(ServiceConfig::default()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("ChainTrail could not start: {e}");
            std::process::exit(1);
        }
    };
    tauri::Builder::default()
        .manage(service)
        .invoke_handler(tauri::generate_handler![
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
