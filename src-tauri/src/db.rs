// db.rs — SQLite setup and all database operations.
//
// We use `rusqlite` which is a thin, synchronous wrapper around SQLite.
// Each Tauri command locks the connection via a Mutex, runs its query,
// and immediately releases the lock — so there's no concurrency issue.

use rusqlite::{Connection, Result, params};
use tauri::AppHandle;
use tauri::Manager;
use std::path::PathBuf;
use chrono::Utc;

use crate::models::{
    CountEntry, Game, GameInput, GameStats, GameStatus, SearchFilter,
    SortField, StatusBreakdown,
};

// ---------------------------------------------------------------------------
// Setup
// ---------------------------------------------------------------------------

/// Resolve the path to games.db inside the OS-appropriate app data directory.
/// e.g. on Windows: C:\Users\<user>\AppData\Roaming\gametrc\games.db
///      on macOS:   ~/Library/Application Support/gametrc/games.db
///      on Linux:   ~/.local/share/gametrc/games.db
pub fn get_db_path(app: &AppHandle) -> PathBuf {
    // RUST NOTE: `unwrap_or_else` is like `unwrap()` but runs a closure if the
    // value is an Err. It's safer than a plain `unwrap()` which would panic.
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("games.db")
}

/// Create all tables and indexes if they don't already exist.
/// `execute_batch` runs multiple SQL statements in one shot.
pub fn init_db(conn: &Connection) -> Result<()> {
    conn.execute_batch("
        PRAGMA journal_mode = WAL;           -- better concurrent read performance
        PRAGMA foreign_keys = ON;            -- enforce FK constraints

        CREATE TABLE IF NOT EXISTS games (
            id                    INTEGER PRIMARY KEY AUTOINCREMENT,
            title                 TEXT    NOT NULL,
            franchise             TEXT,
            sequence_in_franchise INTEGER,
            release_date          TEXT,     -- 'YYYY-MM-DD'
            platform              TEXT    NOT NULL DEFAULT 'PC',
            status                TEXT    NOT NULL DEFAULT 'Backlog',
            progress_percent      REAL    CHECK(progress_percent IS NULL OR
                                                (progress_percent >= 0 AND progress_percent <= 100)),
            playtime_hours        REAL    CHECK(playtime_hours IS NULL OR playtime_hours >= 0),
            rating                REAL    CHECK(rating IS NULL OR (rating >= 1 AND rating <= 10)),
            notes                 TEXT,
            cover_art_path        TEXT,
            developer             TEXT,
            publisher             TEXT,
            created_at            TEXT    NOT NULL,
            updated_at            TEXT    NOT NULL
        );

        -- Screenshots are stored as a separate table (one-to-many)
        CREATE TABLE IF NOT EXISTS game_screenshots (
            id      INTEGER PRIMARY KEY AUTOINCREMENT,
            game_id INTEGER NOT NULL,
            path    TEXT    NOT NULL,
            FOREIGN KEY (game_id) REFERENCES games(id) ON DELETE CASCADE
        );

        -- Genres are stored as a separate table (one-to-many)
        CREATE TABLE IF NOT EXISTS game_genres (
            id      INTEGER PRIMARY KEY AUTOINCREMENT,
            game_id INTEGER NOT NULL,
            genre   TEXT    NOT NULL,
            FOREIGN KEY (game_id) REFERENCES games(id) ON DELETE CASCADE
        );

        -- Indexes for the most common queries
        CREATE INDEX IF NOT EXISTS idx_games_title     ON games(title COLLATE NOCASE);
        CREATE INDEX IF NOT EXISTS idx_games_status    ON games(status);
        CREATE INDEX IF NOT EXISTS idx_games_franchise ON games(franchise);
        CREATE INDEX IF NOT EXISTS idx_games_platform  ON games(platform);
        CREATE INDEX IF NOT EXISTS idx_games_rating    ON games(rating);
    ")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Helper: read a full Game row + its related screenshots and genres
// ---------------------------------------------------------------------------

fn fetch_game_by_id(conn: &Connection, id: i64) -> Result<Option<Game>> {
    let result = conn.query_row(
        "SELECT id, title, franchise, sequence_in_franchise, release_date, platform,
                status, progress_percent, playtime_hours, rating, notes, cover_art_path,
                developer, publisher, created_at, updated_at
         FROM games WHERE id = ?1",
        params![id],
        // RUST NOTE: This closure maps a database row to a Game struct.
        // row.get::<column_index, Type>() extracts a typed column value.
        |row| {
            Ok(Game {
                id:                    row.get(0)?,
                title:                 row.get(1)?,
                franchise:             row.get(2)?,
                sequence_in_franchise: row.get(3)?,
                release_date:          row.get(4)?,
                platform:              row.get(5)?,
                status: GameStatus::from_str(&row.get::<_, String>(6)?),
                progress_percent:      row.get(7)?,
                playtime_hours:        row.get(8)?,
                rating:                row.get(9)?,
                notes:                 row.get(10)?,
                cover_art_path:        row.get(11)?,
                screenshots:           vec![],  // filled below
                developer:             row.get(12)?,
                publisher:             row.get(13)?,
                genres:                vec![],  // filled below
                created_at:            row.get(14)?,
                updated_at:            row.get(15)?,
            })
        },
    );

    match result {
        Ok(mut game) => {
            game.screenshots = fetch_screenshots(conn, id)?;
            game.genres      = fetch_genres(conn, id)?;
            Ok(Some(game))
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

fn fetch_screenshots(conn: &Connection, game_id: i64) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT path FROM game_screenshots WHERE game_id = ?1 ORDER BY id"
    )?;
    // RUST NOTE: `query_map` returns an iterator of Results. We collect them,
    // then use `collect::<Result<Vec<_>, _>>()` to turn Vec<Result<T>> into Result<Vec<T>>.
    let paths = stmt
        .query_map(params![game_id], |row| row.get(0))?
        .collect::<Result<Vec<String>>>()?;
    Ok(paths)
}

fn fetch_genres(conn: &Connection, game_id: i64) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT genre FROM game_genres WHERE game_id = ?1 ORDER BY genre"
    )?;
    let genres = stmt
        .query_map(params![game_id], |row| row.get(0))?
        .collect::<Result<Vec<String>>>()?;
    Ok(genres)
}

// ---------------------------------------------------------------------------
// CRUD operations
// ---------------------------------------------------------------------------

pub fn get_all_games(conn: &Connection) -> Result<Vec<Game>> {
    let mut stmt = conn.prepare(
        "SELECT id FROM games ORDER BY updated_at DESC"
    )?;
    let ids: Vec<i64> = stmt
        .query_map([], |row| row.get(0))?
        .collect::<Result<Vec<i64>>>()?;

    let mut games = Vec::new();
    for id in ids {
        if let Some(game) = fetch_game_by_id(conn, id)? {
            games.push(game);
        }
    }
    Ok(games)
}

pub fn get_game(conn: &Connection, id: i64) -> Result<Option<Game>> {
    fetch_game_by_id(conn, id)
}

pub fn add_game(conn: &Connection, input: GameInput) -> Result<Game> {
    let now = Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO games (title, franchise, sequence_in_franchise, release_date,
            platform, status, progress_percent, playtime_hours, rating, notes,
            cover_art_path, developer, publisher, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            input.title,
            input.franchise,
            input.sequence_in_franchise,
            input.release_date,
            input.platform,
            input.status.as_str(),
            input.progress_percent,
            input.playtime_hours,
            input.rating,
            input.notes,
            input.cover_art_path,
            input.developer,
            input.publisher,
            now,
            now,
        ],
    )?;

    let new_id = conn.last_insert_rowid();
    insert_screenshots(conn, new_id, &input.screenshots)?;
    insert_genres(conn, new_id, &input.genres)?;

    // RUST NOTE: `?` at the end of a Result-returning expression is the "early return
    // on error" operator — equivalent to `unwrap()` but propagates the error to the caller
    // instead of panicking.
    fetch_game_by_id(conn, new_id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)
}

pub fn update_game(conn: &Connection, id: i64, input: GameInput) -> Result<Game> {
    let now = Utc::now().to_rfc3339();

    let rows = conn.execute(
        "UPDATE games SET
            title = ?1, franchise = ?2, sequence_in_franchise = ?3,
            release_date = ?4, platform = ?5, status = ?6, progress_percent = ?7,
            playtime_hours = ?8, rating = ?9, notes = ?10, cover_art_path = ?11,
            developer = ?12, publisher = ?13, updated_at = ?14
         WHERE id = ?15",
        params![
            input.title,
            input.franchise,
            input.sequence_in_franchise,
            input.release_date,
            input.platform,
            input.status.as_str(),
            input.progress_percent,
            input.playtime_hours,
            input.rating,
            input.notes,
            input.cover_art_path,
            input.developer,
            input.publisher,
            now,
            id,
        ],
    )?;

    if rows == 0 {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }

    // Replace related rows: delete old ones, insert new ones
    conn.execute("DELETE FROM game_screenshots WHERE game_id = ?1", params![id])?;
    conn.execute("DELETE FROM game_genres      WHERE game_id = ?1", params![id])?;
    insert_screenshots(conn, id, &input.screenshots)?;
    insert_genres(conn, id, &input.genres)?;

    fetch_game_by_id(conn, id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)
}

pub fn delete_game(conn: &Connection, id: i64) -> Result<bool> {
    let rows = conn.execute("DELETE FROM games WHERE id = ?1", params![id])?;
    Ok(rows > 0)
}

fn insert_screenshots(conn: &Connection, game_id: i64, paths: &[String]) -> Result<()> {
    for path in paths {
        conn.execute(
            "INSERT INTO game_screenshots (game_id, path) VALUES (?1, ?2)",
            params![game_id, path],
        )?;
    }
    Ok(())
}

fn insert_genres(conn: &Connection, game_id: i64, genres: &[String]) -> Result<()> {
    for genre in genres {
        conn.execute(
            "INSERT INTO game_genres (game_id, genre) VALUES (?1, ?2)",
            params![game_id, genre],
        )?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Search & filter
// ---------------------------------------------------------------------------

pub fn search_games(conn: &Connection, filter: SearchFilter) -> Result<Vec<Game>> {
    // We build the SQL query dynamically based on which filters are set.
    // RUST NOTE: `String::new()` creates an empty owned String on the heap.
    let mut conditions: Vec<String> = Vec::new();

    if filter.query.is_some() {
        conditions.push(
            "(g.title LIKE ?_q OR g.franchise LIKE ?_q OR g.notes LIKE ?_q)".to_string()
        );
    }
    if filter.status.is_some()    { conditions.push("g.status = ?_s".to_string()); }
    if filter.platform.is_some()  { conditions.push("g.platform = ?_p".to_string()); }
    if filter.franchise.is_some() { conditions.push("g.franchise LIKE ?_f".to_string()); }
    if filter.genre.is_some() {
        conditions.push(
            "EXISTS (SELECT 1 FROM game_genres gg WHERE gg.game_id = g.id AND gg.genre = ?_g)".to_string()
        );
    }
    if filter.min_rating.is_some() { conditions.push("g.rating >= ?_r".to_string()); }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let order_clause = build_order_clause(&filter);

    // rusqlite doesn't support named params in execute_batch, so we use positional params.
    // Rebuild with clean positional markers:
    let sql = format!(
        "SELECT DISTINCT g.id FROM games g {where_clause} {order_clause}"
    );

    // Collect query parameters in order
    // RUST NOTE: `Box<dyn rusqlite::ToSql>` is a trait object — a dynamically-dispatched
    // value that implements `ToSql`. This lets us mix different types (String, f64, etc.)
    // in a single Vec.
    let mut param_values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    let query_like = filter.query.as_ref().map(|q| format!("%{q}%"));
    let status_str = filter.status.as_ref().map(|s| s.as_str().to_string());
    let franchise_like = filter.franchise.as_ref().map(|f| format!("%{f}%"));

    // Rebuild SQL with real positional params (rusqlite uses ?1, ?2, …)
    let mut param_idx = 1usize;
    let mut final_conditions: Vec<String> = Vec::new();

    if let Some(ref q) = query_like {
        final_conditions.push(format!(
            "(g.title LIKE ?{p} OR g.franchise LIKE ?{p} OR g.notes LIKE ?{p})",
            p = param_idx
        ));
        param_values.push(Box::new(q.clone()));
        param_idx += 1;
    }
    if let Some(ref s) = status_str {
        final_conditions.push(format!("g.status = ?{}", param_idx));
        param_values.push(Box::new(s.clone()));
        param_idx += 1;
    }
    if let Some(ref p) = filter.platform {
        final_conditions.push(format!("g.platform = ?{}", param_idx));
        param_values.push(Box::new(p.clone()));
        param_idx += 1;
    }
    if let Some(ref f) = franchise_like {
        final_conditions.push(format!("g.franchise LIKE ?{}", param_idx));
        param_values.push(Box::new(f.clone()));
        param_idx += 1;
    }
    if let Some(ref g) = filter.genre {
        final_conditions.push(format!(
            "EXISTS (SELECT 1 FROM game_genres gg WHERE gg.game_id = g.id AND gg.genre = ?{})",
            param_idx
        ));
        param_values.push(Box::new(g.clone()));
        param_idx += 1;
    }
    if let Some(r) = filter.min_rating {
        final_conditions.push(format!("g.rating >= ?{}", param_idx));
        param_values.push(Box::new(r));
    }

    let where_str = if final_conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", final_conditions.join(" AND "))
    };

    let final_sql = format!(
        "SELECT DISTINCT g.id FROM games g {where_str} {order_clause}"
    );

    let _ = sql; // suppress unused warning on the earlier draft
    let mut stmt = conn.prepare(&final_sql)?;

    // Convert Vec<Box<dyn ToSql>> to a slice of references for rusqlite
    let params_ref: Vec<&dyn rusqlite::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();
    let ids: Vec<i64> = stmt
        .query_map(params_ref.as_slice(), |row| row.get(0))?
        .collect::<Result<Vec<i64>>>()?;

    let mut games = Vec::new();
    for id in ids {
        if let Some(game) = fetch_game_by_id(conn, id)? {
            games.push(game);
        }
    }
    Ok(games)
}

fn build_order_clause(filter: &SearchFilter) -> String {
    let asc = filter.sort_asc.unwrap_or(true);
    let dir = if asc { "ASC" } else { "DESC" };
    let col = match &filter.sort_by {
        Some(SortField::Title)               => "g.title",
        Some(SortField::ReleaseDate)         => "g.release_date",
        Some(SortField::Rating)              => "g.rating",
        Some(SortField::PlaytimeHours)       => "g.playtime_hours",
        Some(SortField::ProgressPercent)     => "g.progress_percent",
        Some(SortField::SequenceInFranchise) => "g.sequence_in_franchise",
        Some(SortField::UpdatedAt) | None    => "g.updated_at",
    };
    format!("ORDER BY {col} {dir} NULLS LAST")
}

// ---------------------------------------------------------------------------
// Stats
// ---------------------------------------------------------------------------

pub fn get_stats(conn: &Connection) -> Result<GameStats> {
    // Status breakdown
    let mut stmt = conn.prepare(
        "SELECT status, COUNT(*) FROM games GROUP BY status"
    )?;
    let mut breakdown = StatusBreakdown {
        not_started: 0, playing: 0, completed: 0,
        dropped: 0, backlog: 0, wishlist: 0,
    };
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    for row in rows {
        let (status, count) = row?;
        match status.as_str() {
            "NotStarted" => breakdown.not_started = count,
            "Playing"    => breakdown.playing     = count,
            "Completed"  => breakdown.completed   = count,
            "Dropped"    => breakdown.dropped     = count,
            "Backlog"    => breakdown.backlog     = count,
            "Wishlist"   => breakdown.wishlist    = count,
            _            => {}
        }
    }

    let total: i64 = conn.query_row("SELECT COUNT(*) FROM games", [], |r| r.get(0))?;

    let total_playtime: f64 = conn.query_row(
        "SELECT COALESCE(SUM(playtime_hours), 0.0) FROM games", [], |r| r.get(0)
    )?;

    let avg_rating: Option<f64> = conn.query_row(
        "SELECT AVG(rating) FROM games WHERE rating IS NOT NULL", [], |r| r.get(0)
    ).ok().flatten();

    // Completion rate = completed / (total - wishlist) * 100
    let owned = total - breakdown.wishlist;
    let completion_rate = if owned > 0 {
        (breakdown.completed as f64 / owned as f64) * 100.0
    } else {
        0.0
    };

    let games_by_platform = count_by(conn, "SELECT platform, COUNT(*) FROM games GROUP BY platform ORDER BY COUNT(*) DESC")?;
    let games_by_franchise = count_by(conn, "SELECT franchise, COUNT(*) FROM games WHERE franchise IS NOT NULL GROUP BY franchise ORDER BY COUNT(*) DESC LIMIT 20")?;

    // Genre counts come from the many-to-many table
    let mut stmt = conn.prepare(
        "SELECT genre, COUNT(*) AS cnt FROM game_genres GROUP BY genre ORDER BY cnt DESC LIMIT 20"
    )?;
    let games_by_genre = stmt
        .query_map([], |row| {
            Ok(CountEntry { name: row.get(0)?, count: row.get(1)? })
        })?
        .collect::<Result<Vec<_>>>()?;

    // 5 most recently completed games
    let mut stmt = conn.prepare(
        "SELECT title FROM games WHERE status = 'Completed' ORDER BY updated_at DESC LIMIT 5"
    )?;
    let recent_completions: Vec<String> = stmt
        .query_map([], |row| row.get(0))?
        .collect::<Result<Vec<_>>>()?;

    Ok(GameStats {
        total_games: total,
        by_status: breakdown,
        total_playtime_hours: total_playtime,
        average_rating: avg_rating,
        completion_rate,
        games_by_platform,
        games_by_genre,
        games_by_franchise,
        recent_completions,
    })
}

fn count_by(conn: &Connection, sql: &str) -> Result<Vec<CountEntry>> {
    let mut stmt = conn.prepare(sql)?;
    let x = stmt.query_map([], |row| {
        Ok(CountEntry {
            name:  row.get::<_, Option<String>>(0)?.unwrap_or_else(|| "Unknown".to_string()),
            count: row.get(1)?,
        })
    })?
    .collect::<Result<Vec<_>>>(); x
}