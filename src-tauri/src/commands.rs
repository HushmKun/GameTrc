// commands.rs — Tauri command handlers.
//
// These functions are the "API" of your app. The frontend calls them with:
//   import { invoke } from "@tauri-apps/api/core";
//   const games = await invoke("get_all_games");
//
// RUST NOTE: `#[tauri::command]` is a procedural macro that transforms this function
// into something Tauri can call from JavaScript via IPC (inter-process communication).
// Tauri automatically serializes return values to JSON and deserializes arguments from JSON.
//
// `tauri::State<AppState>` is dependency injection — Tauri injects the shared
// application state (our database connection) into each command automatically.

use tauri::State;
use std::sync::Mutex;
use rusqlite::Connection;

use crate::models::{Game, GameInput, GameStats, SearchFilter};
use crate::db;

/// RUST NOTE: This is our shared application state.
/// `Mutex<Connection>` ensures only one thread accesses the DB at a time.
/// Tauri manages multiple threads for IPC, so this is essential.
pub struct AppState {
    pub db: Mutex<Connection>,
}

// ---------------------------------------------------------------------------
// Error handling
// ---------------------------------------------------------------------------

// RUST NOTE: Tauri commands must return `Result<T, E>` where E implements
// `serde::Serialize` so errors can be sent back to JavaScript as JSON.
// `rusqlite::Error` doesn't implement Serialize, so we wrap it in our own type.

#[derive(Debug, serde::Serialize)]
pub struct CommandError(String);

impl From<rusqlite::Error> for CommandError {
    fn from(e: rusqlite::Error) -> Self {
        CommandError(e.to_string())
    }
}

impl From<crate::images::ImageError> for CommandError {
    fn from(e: crate::images::ImageError) -> Self {
        CommandError(e.to_string())
    }
}

// Shorthand type alias — `CmdResult<T>` is `Result<T, CommandError>`
type CmdResult<T> = Result<T, CommandError>;

// Macro to lock the Mutex and propagate the error if poisoned
// RUST NOTE: Mutex::lock() returns a LockResult. If a thread panicked while
// holding the lock it becomes "poisoned". We convert that to our CommandError.
macro_rules! db {
    ($state:expr) => {
        $state
            .db
            .lock()
            .map_err(|e| CommandError(format!("DB lock poisoned: {e}")))?
    };
}

// ---------------------------------------------------------------------------
// Game CRUD
// ---------------------------------------------------------------------------

/// Fetch every game, ordered by most recently updated.
#[tauri::command]
pub fn get_all_games(state: State<AppState>) -> CmdResult<Vec<Game>> {
    let conn = db!(state);
    db::get_all_games(&conn).map_err(Into::into)
}

/// Fetch a single game by its database ID.
#[tauri::command]
pub fn get_game(state: State<AppState>, id: i64) -> CmdResult<Option<Game>> {
    let conn = db!(state);
    db::get_game(&conn, id).map_err(Into::into)
}

/// Insert a new game and return the created record (with its assigned id).
#[tauri::command]
pub fn add_game(state: State<AppState>, input: GameInput) -> CmdResult<Game> {
    let conn = db!(state);
    db::add_game(&conn, input).map_err(Into::into)
}

/// Update an existing game and return the updated record.
#[tauri::command]
pub fn update_game(state: State<AppState>, id: i64, input: GameInput) -> CmdResult<Game> {
    let conn = db!(state);
    db::update_game(&conn, id, input).map_err(Into::into)
}

/// Delete a game. Returns true if a row was deleted, false if id wasn't found.
#[tauri::command]
pub fn delete_game(state: State<AppState>, id: i64) -> CmdResult<bool> {
    let conn = db!(state);
    db::delete_game(&conn, id).map_err(Into::into)
}

// ---------------------------------------------------------------------------
// Search & filter
// ---------------------------------------------------------------------------

/// Search and filter games. All filter fields are optional.
///
/// Example JS call:
///   invoke("search_games", {
///     filter: { query: "zelda", status: "Completed", sort_by: "Rating", sort_asc: false }
///   })
#[tauri::command]
pub fn search_games(state: State<AppState>, filter: SearchFilter) -> CmdResult<Vec<Game>> {
    let conn = db!(state);
    db::search_games(&conn, filter).map_err(Into::into)
}

// ---------------------------------------------------------------------------
// Stats & dashboard
// ---------------------------------------------------------------------------

/// Aggregate statistics for the dashboard.
#[tauri::command]
pub fn get_stats(state: State<AppState>) -> CmdResult<GameStats> {
    let conn = db!(state);
    db::get_stats(&conn).map_err(Into::into)
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

/// Returns all distinct platform names stored in the DB (for filter dropdowns).
#[tauri::command]
pub fn get_platforms(state: State<AppState>) -> CmdResult<Vec<String>> {
    let conn = db!(state);
    let mut stmt = conn
        .prepare("SELECT DISTINCT platform FROM games ORDER BY platform")
        .map_err(|e| CommandError(e.to_string()))?;
    let platforms = stmt
        .query_map([], |row| row.get(0))
        .map_err(|e| CommandError(e.to_string()))?
        .collect::<Result<Vec<String>, _>>()
        .map_err(|e| CommandError(e.to_string()))?;
    Ok(platforms)
}

/// Returns all distinct franchise names (for franchise grouping and autocomplete).
#[tauri::command]
pub fn get_franchises(state: State<AppState>) -> CmdResult<Vec<String>> {
    let conn = db!(state);
    let mut stmt = conn
        .prepare(
            "SELECT DISTINCT franchise FROM games
             WHERE franchise IS NOT NULL ORDER BY franchise"
        )
        .map_err(|e| CommandError(e.to_string()))?;
    let franchises = stmt
        .query_map([], |row| row.get(0))
        .map_err(|e| CommandError(e.to_string()))?
        .collect::<Result<Vec<String>, _>>()
        .map_err(|e| CommandError(e.to_string()))?;
    Ok(franchises)
}

/// Returns all distinct genre names (for filter dropdowns and autocomplete).
#[tauri::command]
pub fn get_genres(state: State<AppState>) -> CmdResult<Vec<String>> {
    let conn = db!(state);
    let mut stmt = conn
        .prepare(
            "SELECT DISTINCT genre FROM game_genres ORDER BY genre"
        )
        .map_err(|e| CommandError(e.to_string()))?;
    let genres = stmt
        .query_map([], |row| row.get(0))
        .map_err(|e| CommandError(e.to_string()))?
        .collect::<Result<Vec<String>, _>>()
        .map_err(|e| CommandError(e.to_string()))?;
    Ok(genres)
}

// ---------------------------------------------------------------------------
// Image processing
// ---------------------------------------------------------------------------

/// Process a cover image: copy a local file or download a remote URL.
///
/// Takes either a local filesystem path or an http(s):// URL.
/// Saves the image to app_data_dir/images/ with a unique filename.
/// Returns the absolute path to the saved image, which should be stored in the DB.
///
/// Example JS call:
///   const savedPath = await invoke("process_cover_image", { input: "https://example.com/cover.jpg" });
///   // or
///   const savedPath = await invoke("process_cover_image", { input: "/home/user/Pictures/game.png" });
#[tauri::command]
pub fn process_cover_image(app: tauri::AppHandle, input: String) -> CmdResult<String> {
    crate::images::process_image(&app, &input).map_err(Into::into)
}