//! The Tauri commands. Register them in the app's `generate_handler!` by this path.

use std::path::PathBuf;

use rhizome_pom::{CommandState, Issue, Ran, Status, View};
use serde_json::Value as Json;
use tauri::{AppHandle, Runtime, State};

use crate::{PomHost, emit};

fn err(e: rhizome_pom::Error) -> String {
    e.to_string()
}

#[tauri::command]
pub fn pom_status(pom: State<'_, PomHost>) -> Status {
    pom.host().status()
}

/// The whole document as rows and schema: what a mirror starts from, and re-reads on a gap
/// in `seq` or when the status's generation moves (decision 48).
#[tauri::command]
pub fn pom_view(pom: State<'_, PomHost>) -> View {
    pom.host().view()
}

/// The whole tree in the file format.
#[tauri::command]
pub fn pom_tree(pom: State<'_, PomHost>) -> String {
    pom.host().tree()
}

/// Every command with its label and whether it can run with `payload`: what the native menu
/// shows.
#[tauri::command]
pub fn pom_commands(pom: State<'_, PomHost>, payload: Option<Json>) -> Vec<CommandState> {
    pom.host().commands(&payload.unwrap_or(Json::Null))
}

/// Runs a command by id. With `coalesce`, consecutive runs with that key are one undo step:
/// a knob drag.
#[tauri::command]
pub fn pom_run<R: Runtime>(
    app: AppHandle<R>,
    pom: State<'_, PomHost>,
    id: String,
    payload: Option<Json>,
    coalesce: Option<String>,
) -> Result<Ran, String> {
    let payload = payload.unwrap_or_else(|| Json::Object(Default::default()));
    let (ran, events) = pom
        .host
        .run(&id, &payload, coalesce.as_deref())
        .map_err(err)?;
    emit(&app, events);
    Ok(ran)
}

/// Opens a gesture: commands run until [`pom_end`] are one undo step. Returns its token.
#[tauri::command]
pub fn pom_begin<R: Runtime>(
    app: AppHandle<R>,
    pom: State<'_, PomHost>,
    label: String,
) -> Result<u64, String> {
    let (token, events) = pom.host().begin(&label).map_err(err)?;
    emit(&app, events);
    Ok(token)
}

#[tauri::command]
pub fn pom_end<R: Runtime>(
    app: AppHandle<R>,
    pom: State<'_, PomHost>,
    token: u64,
) -> Result<(), String> {
    emit(&app, pom.host().end(token).map_err(err)?);
    Ok(())
}

/// Ends a gesture and takes back everything done in it.
#[tauri::command]
pub fn pom_cancel<R: Runtime>(
    app: AppHandle<R>,
    pom: State<'_, PomHost>,
    token: u64,
) -> Result<(), String> {
    emit(&app, pom.host().cancel(token).map_err(err)?);
    Ok(())
}

/// The front end has mounted (or reloaded) and is listening. Ends any gesture it left open,
/// sends the status, and returns a path macOS asked to open before it was listening.
///
/// Call it before restoring the last document: **a file opened from Finder wins over a
/// restored one** (`native-apps.md`, document types, rule 5). Restore only when this returns
/// nothing.
#[tauri::command]
pub fn pom_connect<R: Runtime>(app: AppHandle<R>, pom: State<'_, PomHost>) -> Option<String> {
    emit(&app, pom.host().reset());
    pom.opened.take()
}

#[tauri::command]
pub fn pom_new<R: Runtime>(app: AppHandle<R>, pom: State<'_, PomHost>) -> Result<(), String> {
    emit(&app, pom.host().new_document().map_err(err)?);
    Ok(())
}

/// Opens the file at `path` in place of the current document. Returns what the load couldn't
/// take as written; the document opens anyway.
#[tauri::command]
pub fn pom_open<R: Runtime>(
    app: AppHandle<R>,
    pom: State<'_, PomHost>,
    path: PathBuf,
) -> Result<Vec<Issue>, String> {
    let (issues, events) = pom.host().open(&path).map_err(err)?;
    emit(&app, events);
    Ok(issues)
}

/// Saves to `path` and makes it the document's file. Plain Save is the `file.save` command.
#[tauri::command]
pub fn pom_save_as<R: Runtime>(
    app: AppHandle<R>,
    pom: State<'_, PomHost>,
    path: PathBuf,
) -> Result<(), String> {
    emit(&app, pom.host().save_as(&path).map_err(err)?);
    Ok(())
}
