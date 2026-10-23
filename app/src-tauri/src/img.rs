//! `cinasafe-img://<artwork-id>` (brief 9.5): artwork decrypted in memory
//! and handed to the webview with `Cache-Control: no-store`.

use std::sync::Arc;

use tauri::http::{Request, Response, StatusCode};
use tauri::{Manager, Runtime, UriSchemeContext, UriSchemeResponder};

use crate::state::AppState;

fn reply(status: StatusCode, body: Vec<u8>) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header("Content-Type", "image/jpeg")
        .header("Cache-Control", "no-store")
        .header("Access-Control-Allow-Origin", "*")
        .body(body)
        .unwrap_or_default()
}

fn load(state: &AppState, path: &str) -> Option<Vec<u8>> {
    let id: i64 = path.trim_start_matches('/').split(['?', '/']).next()?.parse().ok()?;
    let vault = state.vault().ok()?;
    let blob = vault.library().artwork(id).ok()?.blob_id;
    let bytes = vault.read_blob(blob).ok()?;
    Some(bytes.to_vec())
}

pub fn handle<R: Runtime>(ctx: UriSchemeContext<'_, R>, request: Request<Vec<u8>>, responder: UriSchemeResponder) {
    let state = ctx.app_handle().state::<Arc<AppState>>().inner().clone();
    let path = request.uri().path().to_owned();
    std::thread::spawn(move || {
        let resp = match load(&state, &path) {
            Some(b) => reply(StatusCode::OK, b),
            None => reply(StatusCode::NOT_FOUND, Vec::new()),
        };
        responder.respond(resp);
    });
}
