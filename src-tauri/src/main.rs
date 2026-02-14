// main.rs — Tauri application entry point.
//
// This file wires everything together:
//   1. Opens / creates the SQLite database
//   2. Registers the Tauri commands so JavaScript can call them
//   3. Starts the Tauri event loop

// RUST NOTE: `mod` declares a module. Rust looks for either
//   src/<name>.rs  or  src/<name>/mod.rs
// These four modules live in the src/ folder as separate .rs files.
#![windows_subsystem = "windows"]
mod models;
mod db;
mod commands;

use tauri::Manager;
use std::sync::Mutex;
use rusqlite::Connection;

// Re-export AppState from commands so db.rs can stay clean
use commands::AppState;

fn main() {
    tauri::Builder::default()
        // ── Plugins ──────────────────────────────────────────────────────────
        // tauri-plugin-dialog lets Rust/JS open native file picker dialogs
        .plugin(tauri_plugin_dialog::init())
        // tauri-plugin-fs gives the frontend safe access to the filesystem
        .plugin(tauri_plugin_fs::init())
        
        // ── One-time setup ───────────────────────────────────────────────────
        .setup(|app| {
            // Resolve the OS-standard data directory and open our SQLite DB
            let db_path = db::get_db_path(app.handle());

            // Create parent directories if they don't exist yet
            // RUST NOTE: `unwrap()` panics if the Result is Err. During setup
            // a panic is acceptable — if we can't create the data dir, the app
            // cannot function at all.
            std::fs::create_dir_all(db_path.parent().unwrap())
                .expect("Failed to create app data directory");

            let conn = Connection::open(&db_path)
                .expect("Failed to open SQLite database");

            // Run CREATE TABLE IF NOT EXISTS migrations
            db::init_db(&conn)
                .expect("Failed to initialise database schema");

            // Register shared state — available in every command via State<AppState>
            // RUST NOTE: `Mutex::new(conn)` wraps the Connection in a mutex so it
            // can be safely shared across threads.
            app.manage(AppState { db: Mutex::new(conn) });

            Ok(())
        })

        // ── Register IPC commands ────────────────────────────────────────────
        // Every function listed here can be called from JavaScript with:
        //   import { invoke } from "@tauri-apps/api/core";
        //   invoke("command_name", { arg: value })
        .invoke_handler(tauri::generate_handler![
            // CRUD
            commands::get_all_games,
            commands::get_game,
            commands::add_game,
            commands::update_game,
            commands::delete_game,
            // Search
            commands::search_games,
            // Stats
            commands::get_stats,
            // Utility / dropdowns
            commands::get_platforms,
            commands::get_franchises,
            commands::get_genres,
        ])

        // ── Start the event loop ─────────────────────────────────────────────
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}