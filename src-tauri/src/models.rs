// models.rs — All data types for the GameTrc.
//
// RUST NOTE: `derive` macros auto-generate trait implementations for us.
//   - `Serialize / Deserialize` (from serde) let Tauri automatically convert
//     these structs to/from JSON when passing data between Rust and the frontend.
//   - `Debug`   lets you print them with `{:?}` for logging.
//   - `Clone`   lets you duplicate a value (Rust moves by default, unlike most languages).

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

/// Tracks where the player is in their journey with a game.
/// RUST NOTE: Rust enums are algebraic — they can carry data — but here we use
/// simple variants, similar to an enum in C# or Java.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum GameStatus {
    NotStarted,
    Playing,
    Completed,
    Dropped,
    Backlog,    // owned but not started yet
    Wishlist,   // want but don't own
}

impl GameStatus {
    /// Convert to a string for SQLite storage.
    /// RUST NOTE: `&str` is a string slice (borrowed reference to string data).
    /// `String` is an owned, heap-allocated string. We return `&str` here
    /// because we're pointing at static string literals — no allocation needed.
    pub fn as_str(&self) -> &str {
        match self {
            GameStatus::NotStarted => "NotStarted",
            GameStatus::Playing    => "Playing",
            GameStatus::Completed  => "Completed",
            GameStatus::Dropped    => "Dropped",
            GameStatus::Backlog    => "Backlog",
            GameStatus::Wishlist   => "Wishlist",
        }
    }

    /// Parse from a string coming out of SQLite.
    pub fn from_str(s: &str) -> Self {
        match s {
            "Playing"    => GameStatus::Playing,
            "Completed"  => GameStatus::Completed,
            "Dropped"    => GameStatus::Dropped,
            "Backlog"    => GameStatus::Backlog,
            "Wishlist"   => GameStatus::Wishlist,
            _            => GameStatus::NotStarted,
        }
    }
}

// ---------------------------------------------------------------------------
// Core game record — returned to the frontend
// ---------------------------------------------------------------------------

/// Full game record, including DB-generated fields.
/// RUST NOTE: `Option<T>` is Rust's null-safety type. A field typed `Option<String>`
/// is either `Some("value")` or `None` — there is no null pointer.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Game {
    pub id:                       i64,
    pub title:                    String,
    pub franchise:                Option<String>,
    pub sequence_in_franchise:    Option<i32>,
    pub release_date:             Option<String>,   // stored as "YYYY-MM-DD"
    pub platform:                 String,
    pub status:                   GameStatus,
    pub progress_percent:         Option<f64>,      // 0.0 – 100.0
    pub playtime_hours:           Option<f64>,
    pub rating:                   Option<f64>,      // 1.0 – 10.0
    pub notes:                    Option<String>,
    pub cover_art_path:           Option<String>,
    pub screenshots:              Vec<String>,       // list of file paths
    pub developer:                Option<String>,
    pub publisher:                Option<String>,
    pub genres:                   Vec<String>,
    pub created_at:               String,           // ISO 8601
    pub updated_at:               String,
}

// ---------------------------------------------------------------------------
// Input structs — received from the frontend (no id / timestamps)
// ---------------------------------------------------------------------------

/// Used when creating or updating a game. The frontend sends this JSON payload.
#[derive(Debug, Serialize, Deserialize)]
pub struct GameInput {
    pub title:                    String,
    pub franchise:                Option<String>,
    pub sequence_in_franchise:    Option<i32>,
    pub release_date:             Option<String>,
    pub platform:                 String,
    pub status:                   GameStatus,
    pub progress_percent:         Option<f64>,
    pub playtime_hours:           Option<f64>,
    pub rating:                   Option<f64>,
    pub notes:                    Option<String>,
    pub cover_art_path:           Option<String>,
    pub screenshots:              Vec<String>,
    pub developer:                Option<String>,
    pub publisher:                Option<String>,
    pub genres:                   Vec<String>,
}

// ---------------------------------------------------------------------------
// Filter / search
// ---------------------------------------------------------------------------

/// All fields are optional — the frontend sends only the ones it wants to filter by.
#[derive(Debug, Serialize, Deserialize)]
pub struct SearchFilter {
    pub query:     Option<String>,      // searches title, franchise, notes
    pub status:    Option<GameStatus>,
    pub platform:  Option<String>,
    pub franchise: Option<String>,
    pub genre:     Option<String>,
    pub min_rating: Option<f64>,
    pub sort_by:   Option<SortField>,
    pub sort_asc:  Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum SortField {
    Title,
    ReleaseDate,
    Rating,
    PlaytimeHours,
    ProgressPercent,
    UpdatedAt,
    SequenceInFranchise,
}

// ---------------------------------------------------------------------------
// Stats / dashboard
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct GameStats {
    pub total_games:          i64,
    pub by_status:            StatusBreakdown,
    pub total_playtime_hours: f64,
    pub average_rating:       Option<f64>,
    pub completion_rate:      f64,              // % of non-wishlist games completed
    pub games_by_platform:    Vec<CountEntry>,
    pub games_by_genre:       Vec<CountEntry>,
    pub games_by_franchise:   Vec<CountEntry>,
    pub recent_completions:   Vec<String>,      // titles of recently completed games
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StatusBreakdown {
    pub not_started: i64,
    pub playing:     i64,
    pub completed:   i64,
    pub dropped:     i64,
    pub backlog:     i64,
    pub wishlist:    i64,
}

/// A generic name → count pair used for chart data.
#[derive(Debug, Serialize, Deserialize)]
pub struct CountEntry {
    pub name:  String,
    pub count: i64,
}