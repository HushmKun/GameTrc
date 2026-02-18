// app.js — Game Tracker Frontend
//
// Requires `"withGlobalTauri": true` in tauri.conf.json (under app.security).
// Tauri then injects its full API onto window.__TAURI__ before the page loads,
// so no ES module imports are needed — works with a plain <script src="app.js">.
//
// API surface used:
//   window.__TAURI__.core.invoke(cmd, args)  — calls a Rust #[tauri::command]
//   window.__TAURI__.dialog.open(options)    — native file-picker dialog (cover art)

// ── Grab globals once, fail loudly if Tauri isn't present ────────────────────
if (!window.__TAURI__) {
  throw new Error(
    '[Game Tracker] window.__TAURI__ is not defined. ' +
    'Make sure "withGlobalTauri": true is set in tauri.conf.json under app.security.'
  );
}

const { invoke, convertFileSrc } = window.__TAURI__.core;
const { open: openDialog }       = window.__TAURI__.dialog;


// =============================================================================
// State
// =============================================================================

const state = {
  games: [],
  allPlatforms: [],
  allFranchises: [],
  allGenres: [],
  currentFilter: {
    query: "", status: "", platform: "", franchise: "",
    genre: "", sortBy: "UpdatedAt", sortAsc: false,
  },
  activeView: "library",  // "library" | "backlog" | "wishlist" | "stats"
  selectedGameId: null,
  editingGameId: null,    // null = adding new
  pendingDeleteId: null,
  formGenres: [],
  formRating: null,
  isListView: false,
};


// =============================================================================
// Helpers
// =============================================================================

const $ = id => document.getElementById(id);

function statusClass(status) {
  return {
    Playing: "status--playing", Completed: "status--completed",
    Dropped: "status--dropped", Backlog: "status--backlog",
    Wishlist: "status--wishlist", NotStarted: "status--notstarted",
  }[status] ?? "status--notstarted";
}

function statusLabel(status) {
  return {
    NotStarted: "Not Started", Playing: "Playing", Completed: "Completed",
    Dropped: "Dropped", Backlog: "Backlog", Wishlist: "Wishlist",
  }[status] ?? status;
}

function fmtRating(r) { return r != null ? `★ ${r.toFixed(1)}` : "—"; }
function fmtHours(h)  { return h != null ? `${h}h` : "—"; }
function fmtDate(d)   {
  return d
    ? new Date(d).toLocaleDateString("en-US", { year: "numeric", month: "short", day: "numeric" })
    : "—";
}

function resolveCover(path) {
  if (!path) return null;
  // Already a remote URL — use as-is
  if (path.startsWith("http://") || path.startsWith("https://")) return path;
  // Local filesystem path — convert for the asset protocol
  return convertFileSrc(path);
}

function showToast(msg, type = "info") {
  const el = document.createElement("div");
  el.className = `toast ${type}`;
  el.innerHTML = `<span class="toast-dot"></span>${msg}`;
  $("toastContainer").appendChild(el);
  setTimeout(() => {
    el.classList.add("exiting");
    setTimeout(() => el.remove(), 300);
  }, 2800);
}

function applyStagger(cards) {
  cards.forEach((c, i) => { c.style.animationDelay = `${i * 0.03}s`; });
}

const CHART_COLORS = ["#f59e0b", "#3b82f6", "#22c55e", "#a855f7", "#f97316", "#ec4899", "#06b6d4"];


// =============================================================================
// API calls  (all delegate straight to Rust via IPC)
// =============================================================================

async function loadGames() {
  const f = state.currentFilter;
  try {
    state.games = await invoke("search_games", {
      filter: {
        query:     f.query     || null,
        status:    f.status    || null,
        platform:  f.platform  || null,
        franchise: f.franchise || null,
        genre:     f.genre     || null,
        sort_by:   f.sortBy,
        sort_asc:  f.sortAsc,
      },
    });
  } catch (e) {
    console.error("search_games failed:", e);
    showToast("Failed to load games", "error");
    state.games = [];
  }
}

async function loadMeta() {
  try {
    [state.allPlatforms, state.allFranchises, state.allGenres] = await Promise.all([
      invoke("get_platforms"),
      invoke("get_franchises"),
      invoke("get_genres"),
    ]);
  } catch (e) {
    console.error("loadMeta failed:", e);
  }
  renderPlatformFilters();
  renderGenreFilters();
  populateDataLists();
}


// =============================================================================
// Render: Game Grid / List
// =============================================================================

function renderGames() {
  const grid  = $("gameGrid");
  const empty = $("emptyState");
  const count = $("gameCount");

  // Secondary view-level filter on top of Rust search results
  let games = state.games;
  if (state.activeView === "backlog")  games = games.filter(g => g.status === "Backlog");
  if (state.activeView === "wishlist") games = games.filter(g => g.status === "Wishlist");

  count.textContent = `${games.length} game${games.length !== 1 ? "s" : ""}`;

  if (games.length === 0) {
    grid.innerHTML = "";
    empty.classList.remove("hidden");
    return;
  }
  empty.classList.add("hidden");

  grid.className = `game-grid${state.isListView ? " list-view" : ""}`;
  grid.innerHTML = games.map(g => state.isListView ? renderListCard(g) : renderGridCard(g)).join("");
  applyStagger([...grid.children]);

  grid.querySelectorAll(".game-card").forEach(card => {
    const id = Number(card.dataset.id);
    card.addEventListener("click", (e) => {
      if (e.target.closest(".icon-btn")) return;
      openDetail(id);
    });
  });
  grid.querySelectorAll("[data-edit]").forEach(btn => {
    btn.addEventListener("click", () => openModal(Number(btn.dataset.edit)));
  });
  grid.querySelectorAll("[data-delete]").forEach(btn => {
    btn.addEventListener("click", () => promptDelete(Number(btn.dataset.delete)));
  });
}

function renderGridCard(g) {
  const coverHtml = g.cover_art_path
    ? `<img src="${resolveCover(g.cover_art_path)}" alt="${g.title}" loading="lazy" />`
    : `<div class="card-cover-placeholder"><span class="cover-letter">${g.title.charAt(0).toUpperCase()}</span></div>`;

  const progressHtml = g.progress_percent != null
    ? `<div class="card-progress-bar"><div class="card-progress-fill" style="width:${g.progress_percent}%"></div></div>`
    : "";

  return `
    <div class="game-card" data-id="${g.id}">
      <div class="card-cover">
        ${coverHtml}
        <span class="status-badge ${statusClass(g.status)}">${statusLabel(g.status)}</span>
      </div>
      <div class="card-info">
        <div class="card-title" title="${g.title}">${g.title}</div>
        <div class="card-meta">
          <span class="card-platform">${g.platform}</span>
          <span class="card-rating">${fmtRating(g.rating)}</span>
        </div>
        ${progressHtml}
      </div>
    </div>`;
}

function renderListCard(g) {
  const coverHtml = g.cover_art_path
    ? `<img src="${resolveCover(g.cover_art_path)}" alt="${g.title}" loading="lazy" />`
    : `<div class="card-cover-placeholder"><span class="cover-letter">${g.title.charAt(0).toUpperCase()}</span></div>`;

  return `
    <div class="game-card list-card" data-id="${g.id}">
      <div class="card-cover">${coverHtml}</div>
      <div class="card-info">
        <div class="card-title">${g.title}</div>
        <div class="card-meta">
          <span class="card-platform">${g.platform}</span>
          ${g.franchise ? `<span class="card-platform">· ${g.franchise}</span>` : ""}
        </div>
      </div>
      <div class="list-card-right">
        <span class="status-badge ${statusClass(g.status)}" style="position:static">${statusLabel(g.status)}</span>
        <span class="card-rating" style="font-family:var(--font-mono);font-size:11px">${fmtRating(g.rating)}</span>
        <span class="card-rating" style="font-family:var(--font-mono);font-size:11px;color:var(--text-3)">${fmtHours(g.playtime_hours)}</span>
        <div class="list-card-actions">
          <button class="icon-btn" data-edit="${g.id}" title="Edit">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
              <path d="M11 5H6a2 2 0 00-2 2v11a2 2 0 002 2h11a2 2 0 002-2v-5m-1.414-9.414a2 2 0 112.828 2.828L11.828 15H9v-2.828l8.586-8.586z"/>
            </svg>
          </button>
          <button class="icon-btn danger" data-delete="${g.id}" title="Delete">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
              <path d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16"/>
            </svg>
          </button>
        </div>
      </div>
    </div>`;
}


// =============================================================================
// Detail Panel
// =============================================================================

function openDetail(id) {
  const game = state.games.find(g => g.id === id);
  if (!game) return;
  state.selectedGameId = id;

  const coverSrc   = game.cover_art_path ? resolveCover(game.cover_art_path) : null;
  const coverHtml  = coverSrc
    ? `<img src="${coverSrc}" alt="${game.title}" />`
    : `<div class="detail-cover-placeholder"><span class="cover-letter">${game.title.charAt(0).toUpperCase()}</span></div>`;

  const genreTags      = game.genres.map(g => `<span class="detail-tag">${g}</span>`).join("");
  const screenshotsHtml = game.screenshots.length
    ? `<div class="detail-screenshots">${game.screenshots.map(s =>
        `<div class="detail-screenshot"><img src="${resolveCover(s)}" /></div>`).join("")}</div>`
    : "";
  const notesHtml = game.notes ? `<div class="detail-notes">${game.notes}</div>` : "";

  $("detailContent").innerHTML = `
    <div class="detail-cover">${coverHtml}</div>
    <h2 class="detail-title">${game.title}</h2>
    ${game.franchise
      ? `<p class="detail-franchise">${game.franchise}${game.sequence_in_franchise ? ` · #${game.sequence_in_franchise}` : ""}</p>`
      : ""}
    <div class="detail-status-row">
      <span class="status-badge ${statusClass(game.status)}" style="position:static">${statusLabel(game.status)}</span>
      ${game.rating != null
        ? `<span style="font-family:var(--font-mono);font-size:12px;color:var(--accent)">★ ${game.rating.toFixed(1)}</span>`
        : ""}
    </div>
    ${game.progress_percent != null ? `
    <div class="detail-progress">
      <label>Progress <span>${game.progress_percent.toFixed(0)}%</span></label>
      <div class="progress-bar"><div class="progress-fill" style="width:0%"></div></div>
    </div>` : ""}
    <div class="detail-meta-grid">
      <div class="detail-meta-item"><label>Platform</label><span>${game.platform}</span></div>
      <div class="detail-meta-item"><label>Playtime</label><span>${fmtHours(game.playtime_hours)}</span></div>
      <div class="detail-meta-item"><label>Release</label><span>${fmtDate(game.release_date)}</span></div>
      <div class="detail-meta-item"><label>Developer</label><span>${game.developer || "—"}</span></div>
      <div class="detail-meta-item"><label>Publisher</label><span>${game.publisher || "—"}</span></div>
      <div class="detail-meta-item"><label>Added</label><span>${fmtDate(game.created_at)}</span></div>
    </div>
    ${genreTags ? `<div class="detail-tags">${genreTags}</div>` : ""}
    ${notesHtml}
    ${screenshotsHtml}
    <div class="detail-actions">
      <button class="btn btn--ghost" id="detailEditBtn">Edit</button>
      <button class="btn btn--danger" id="detailDeleteBtn" style="flex:unset">Delete</button>
    </div>`;

  requestAnimationFrame(() => {
    const fill = $("detailContent").querySelector(".progress-fill");
    if (fill && game.progress_percent != null) {
      setTimeout(() => { fill.style.width = `${game.progress_percent}%`; }, 80);
    }
  });

  $("detailPanel").classList.add("open");
  $("overlay").classList.add("active");

  $("detailContent").querySelector("#detailEditBtn")?.addEventListener("click", () => {
    closeDetail();
    openModal(id);
  });
  $("detailContent").querySelector("#detailDeleteBtn")?.addEventListener("click", () => {
    closeDetail();
    promptDelete(id);
  });
}

function closeDetail() {
  $("detailPanel").classList.remove("open");
  $("overlay").classList.remove("active");
  state.selectedGameId = null;
}


// =============================================================================
// Sidebar Filter Rendering
// =============================================================================

function renderPlatformFilters() {
  const wrap = $("platformFilters");
  wrap.innerHTML = `<button class="chip ${!state.currentFilter.platform ? "active" : ""}" data-platform="">All</button>`;
  state.allPlatforms.forEach(p => {
    const btn = document.createElement("button");
    btn.className = `chip ${state.currentFilter.platform === p ? "active" : ""}`;
    btn.dataset.platform = p;
    btn.textContent = p;
    wrap.appendChild(btn);
  });
  wrap.querySelectorAll(".chip").forEach(btn => {
    btn.addEventListener("click", () => {
      state.currentFilter.platform = btn.dataset.platform;
      wrap.querySelectorAll(".chip").forEach(b => b.classList.toggle("active", b === btn));
      refresh();
    });
  });
}

function renderGenreFilters() {
  const wrap = $("genreFilters");
  wrap.innerHTML = `<button class="chip ${!state.currentFilter.genre ? "active" : ""}" data-genre="">All</button>`;
  state.allGenres.slice(0, 12).forEach(g => {
    const btn = document.createElement("button");
    btn.className = `chip ${state.currentFilter.genre === g ? "active" : ""}`;
    btn.dataset.genre = g;
    btn.textContent = g;
    wrap.appendChild(btn);
  });
  wrap.querySelectorAll(".chip").forEach(btn => {
    btn.addEventListener("click", () => {
      state.currentFilter.genre = btn.dataset.genre;
      wrap.querySelectorAll(".chip").forEach(b => b.classList.toggle("active", b === btn));
      refresh();
    });
  });
}

function populateDataLists() {
  $("franchiseList").innerHTML = state.allFranchises.map(f => `<option value="${f}">`).join("");
  $("platformList").innerHTML  = state.allPlatforms.map(p => `<option value="${p}">`).join("");
  $("genreList").innerHTML     = state.allGenres.map(g => `<option value="${g}">`).join("");
}


// =============================================================================
// Stats View
// =============================================================================

async function renderStats() {
  const grid = $("statsGrid");
  grid.innerHTML = `<div class="stat-card"><div class="stat-label">Loading…</div></div>`;

  let stats;
  try {
    stats = await invoke("get_stats");
  } catch (e) {
    console.error("get_stats failed:", e);
    grid.innerHTML = `<div class="stat-card"><div class="stat-label">Error loading stats</div></div>`;
    return;
  }

  const barList = (items, maxCount) => items.map((item, i) => `
    <div class="chart-bar-item">
      <div class="chart-bar-header">
        <span>${item.name}</span>
        <span class="num">${item.count}</span>
      </div>
      <div class="chart-bar-track">
        <div class="chart-bar-fill"
          data-w="${(item.count / maxCount * 100).toFixed(0)}"
          style="width:0%;background:${CHART_COLORS[i % CHART_COLORS.length]}">
        </div>
      </div>
    </div>`).join("");

  const maxPlatform  = Math.max(1, ...stats.games_by_platform.map(x => x.count));
  const maxGenre     = Math.max(1, ...stats.games_by_genre.map(x => x.count));
  const maxFranchise = Math.max(1, ...stats.games_by_franchise.map(x => x.count));

  grid.innerHTML = `
    <div class="stat-card" style="animation-delay:0s">
      <div class="stat-label">Total Games</div>
      <div class="stat-value">${stats.total_games}</div>
    </div>
    <div class="stat-card" style="animation-delay:0.05s">
      <div class="stat-label">Total Playtime</div>
      <div class="stat-value">${Math.round(stats.total_playtime_hours)}<span style="font-size:16px;color:var(--text-3)">h</span></div>
    </div>
    <div class="stat-card" style="animation-delay:0.1s">
      <div class="stat-label">Avg Rating</div>
      <div class="stat-value" style="color:var(--accent)">
        ${stats.average_rating != null ? stats.average_rating.toFixed(1) : "—"}
      </div>
    </div>
    <div class="stat-card" style="animation-delay:0.15s">
      <div class="stat-label">Completion Rate</div>
      <div class="stat-value">${stats.completion_rate.toFixed(0)}<span style="font-size:16px;color:var(--text-3)">%</span></div>
      <div class="stat-sub">of owned games</div>
    </div>

    <div class="stat-card stat-card--wide" style="animation-delay:0.2s">
      <div class="stat-label">By Status</div>
      <div class="status-breakdown">
        <div class="breakdown-item"><span class="label">Playing</span>   <span class="count" style="color:var(--blue)">${stats.by_status.playing}</span></div>
        <div class="breakdown-item"><span class="label">Completed</span> <span class="count" style="color:var(--green)">${stats.by_status.completed}</span></div>
        <div class="breakdown-item"><span class="label">Backlog</span>   <span class="count" style="color:var(--purple)">${stats.by_status.backlog}</span></div>
        <div class="breakdown-item"><span class="label">Wishlist</span>  <span class="count" style="color:var(--orange)">${stats.by_status.wishlist}</span></div>
        <div class="breakdown-item"><span class="label">Dropped</span>   <span class="count" style="color:var(--red)">${stats.by_status.dropped}</span></div>
        <div class="breakdown-item"><span class="label">Not Started</span><span class="count" style="color:var(--text-3)">${stats.by_status.not_started}</span></div>
      </div>
    </div>

    ${stats.games_by_platform.length ? `
    <div class="stat-card" style="animation-delay:0.25s">
      <div class="stat-label">By Platform</div>
      <div class="chart-bar-list">${barList(stats.games_by_platform, maxPlatform)}</div>
    </div>` : ""}

    ${stats.games_by_genre.length ? `
    <div class="stat-card" style="animation-delay:0.3s">
      <div class="stat-label">Top Genres</div>
      <div class="chart-bar-list">${barList(stats.games_by_genre, maxGenre)}</div>
    </div>` : ""}

    ${stats.games_by_franchise.length ? `
    <div class="stat-card" style="animation-delay:0.35s">
      <div class="stat-label">By Franchise</div>
      <div class="chart-bar-list">${barList(stats.games_by_franchise, maxFranchise)}</div>
    </div>` : ""}

    ${stats.recent_completions.length ? `
    <div class="stat-card" style="animation-delay:0.4s">
      <div class="stat-label">Recently Completed</div>
      <div style="margin-top:10px;display:flex;flex-direction:column;gap:7px">
        ${stats.recent_completions.map(t => `
          <div style="font-size:13px;color:var(--text-2);display:flex;align-items:center;gap:8px">
            <span style="color:var(--green);font-size:14px">✓</span>${t}
          </div>`).join("")}
      </div>
    </div>` : ""}
  `;

  requestAnimationFrame(() => {
    setTimeout(() => {
      grid.querySelectorAll(".chart-bar-fill[data-w]").forEach(el => {
        el.style.width = el.dataset.w + "%";
      });
    }, 100);
  });
}


// =============================================================================
// Add / Edit Modal
// =============================================================================

function openModal(id = null) {
  state.editingGameId = id;
  $("modalTitle").textContent = id ? "Edit Game" : "Add Game";
  $("submitBtn").textContent  = id ? "Save Changes" : "Save Game";

  const form = $("gameForm");
  form.reset();
  state.formGenres = [];
  state.formRating = null;
  renderFormGenres();
  renderStars();
  $("coverPreview").classList.add("hidden");
  $("coverPlaceholder").classList.remove("hidden");
  $("progressVal").textContent = "0%";
  $("coverUrlInput").value = "";

  // Reset cover art tabs to Upload
  document.querySelectorAll(".cover-tab").forEach(t => t.classList.remove("active"));
  document.querySelector(".cover-tab[data-tab='upload']").classList.add("active");
  document.querySelectorAll(".cover-tab-content").forEach(c => c.classList.remove("active"));
  $("uploadTab").classList.add("active");

  $("statusPicker").querySelectorAll(".status-opt").forEach(b => {
    b.classList.toggle("active", b.dataset.val === "NotStarted");
  });

  if (id) {
    const game = state.games.find(g => g.id === id);
    if (!game) return;
    $("f_title").value            = game.title;
    $("f_franchise").value        = game.franchise || "";
    $("f_sequence").value         = game.sequence_in_franchise || "";
    $("f_platform").value         = game.platform;
    $("f_release_date").value     = game.release_date || "";
    $("f_developer").value        = game.developer || "";
    $("f_publisher").value        = game.publisher || "";
    $("f_progress").value         = game.progress_percent ?? 0;
    $("progressVal").textContent  = `${Math.round(game.progress_percent ?? 0)}%`;
    $("f_playtime").value         = game.playtime_hours ?? "";
    $("f_notes").value            = game.notes || "";
    $("f_cover_art_path").value   = game.cover_art_path || "";

    if (game.cover_art_path) {
      // Check if it's a URL or local path and switch tabs accordingly
      const isUrl = game.cover_art_path.startsWith("http://") || game.cover_art_path.startsWith("https://");
      
      if (isUrl) {
        // Switch to URL tab
        document.querySelectorAll(".cover-tab").forEach(t => t.classList.remove("active"));
        document.querySelector(".cover-tab[data-tab='url']").classList.add("active");
        document.querySelectorAll(".cover-tab-content").forEach(c => c.classList.remove("active"));
        $("urlTab").classList.add("active");
        $("coverUrlInput").value = game.cover_art_path;
      }
      
      $("coverPreview").src = resolveCover(game.cover_art_path);
      $("coverPreview").classList.remove("hidden");
      $("coverPlaceholder").classList.add("hidden");
    }

    $("statusPicker").querySelectorAll(".status-opt").forEach(b => {
      b.classList.toggle("active", b.dataset.val === game.status);
    });

    state.formGenres = [...game.genres];
    renderFormGenres();

    if (game.rating != null) {
      state.formRating = game.rating;
      renderStars(game.rating);
    }
  }

  $("modalBackdrop").classList.add("open");
}

function closeModal() {
  $("modalBackdrop").classList.remove("open");
  state.editingGameId = null;
}

// Status picker
$("statusPicker").addEventListener("click", e => {
  const btn = e.target.closest(".status-opt");
  if (!btn) return;
  $("statusPicker").querySelectorAll(".status-opt").forEach(b => b.classList.remove("active"));
  btn.classList.add("active");
});

// Progress slider
$("f_progress").addEventListener("input", e => {
  $("progressVal").textContent = `${e.target.value}%`;
});

// Star rating
function renderStars(val = state.formRating) {
  $("starRating").querySelectorAll(".star").forEach(s => {
    s.classList.toggle("active", parseFloat(s.dataset.v) <= (val ?? 0));
  });
  $("f_rating").value = val ?? "";
}

$("starRating").addEventListener("click", e => {
  const star = e.target.closest(".star");
  if (!star) return;
  const v = parseFloat(star.dataset.v);
  state.formRating = state.formRating === v ? null : v;
  renderStars();
});
$("starRating").addEventListener("mouseover", e => {
  const star = e.target.closest(".star");
  if (!star) return;
  renderStars(parseFloat(star.dataset.v));
});
$("starRating").addEventListener("mouseleave", () => renderStars());

// Genre tags
function renderFormGenres() {
  const list = $("genreTags");
  list.innerHTML = state.formGenres.map(g => `
    <span class="tag">${g}<span class="tag-remove" data-genre="${g}">×</span></span>`).join("");
  list.querySelectorAll(".tag-remove").forEach(btn => {
    btn.addEventListener("click", () => {
      state.formGenres = state.formGenres.filter(x => x !== btn.dataset.genre);
      renderFormGenres();
    });
  });
}

$("genreInput").addEventListener("keydown", e => {
  if (e.key === "Enter" || e.key === ",") {
    e.preventDefault();
    const val = $("genreInput").value.trim();
    if (val && !state.formGenres.includes(val)) {
      state.formGenres.push(val);
      renderFormGenres();
    }
    $("genreInput").value = "";
  }
});
$("genreTagWrap").addEventListener("click", () => $("genreInput").focus());

// Cover art tabs
document.querySelectorAll(".cover-tab").forEach(tab => {
  tab.addEventListener("click", () => {
    const targetTab = tab.dataset.tab;
    document.querySelectorAll(".cover-tab").forEach(t => t.classList.remove("active"));
    tab.classList.add("active");
    document.querySelectorAll(".cover-tab-content").forEach(c => c.classList.remove("active"));
    $(targetTab === "upload" ? "uploadTab" : "urlTab").classList.add("active");
  });
});

// Load image from URL
$("loadUrlBtn").addEventListener("click", async () => {
  const url = $("coverUrlInput").value.trim();
  if (!url) {
    showToast("Please enter a URL", "error");
    return;
  }
  
  if (!url.startsWith("http://") && !url.startsWith("https://")) {
    showToast("URL must start with http:// or https://", "error");
    return;
  }
  
  $("loadUrlBtn").disabled = true;
  $("loadUrlBtn").textContent = "Loading...";
  
  try {
    // Just store the URL for now — it will be processed on form submit
    $("f_cover_art_path").value = url;
    $("coverPreview").src = url; // Show preview directly for remote URLs
    $("coverPreview").classList.remove("hidden");
    $("coverPlaceholder").classList.add("hidden");
    showToast("Image loaded", "success");
  } catch (e) {
    console.error("Failed to load URL:", e);
    showToast("Failed to load image", "error");
  } finally {
    $("loadUrlBtn").disabled = false;
    $("loadUrlBtn").textContent = "Load Image";
  }
});

// Cover art — use Tauri's native dialog instead of a hidden <input type="file">
$("coverDrop").addEventListener("click", async () => {
  try {
    // openDialog returns the selected file path as a string, or null if cancelled.
    const selected = await openDialog({
      multiple: false,
      filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp", "gif"] }],
    });
    if (!selected) return;

    // `selected` is the absolute path on disk
    $("f_cover_art_path").value = selected;
    $("coverPreview").src = resolveCover(selected);
    $("coverPreview").classList.remove("hidden");
    $("coverPlaceholder").classList.add("hidden");
  } catch (e) {
    console.error("File dialog failed:", e);
    showToast("Could not open file picker", "error");
  }
});

// Also support drag-and-drop (Tauri exposes the real path via the dataTransfer API)
$("coverDrop").addEventListener("dragover", e => {
  e.preventDefault();
  $("coverDrop").style.borderColor = "var(--accent)";
});
$("coverDrop").addEventListener("dragleave", () => {
  $("coverDrop").style.borderColor = "";
});
$("coverDrop").addEventListener("drop", async e => {
  e.preventDefault();
  $("coverDrop").style.borderColor = "";
  const file = e.dataTransfer.files[0];
  if (!file || !file.type.startsWith("image/")) return;

  // In Tauri v2 the dropped File object exposes `.path` (absolute FS path).
  const path = file.path ?? file.name;
  $("f_cover_art_path").value = path;

  const reader = new FileReader();
  reader.onload = ev => {
    $("coverPreview").src = ev.target.result;
    $("coverPreview").classList.remove("hidden");
    $("coverPlaceholder").classList.add("hidden");
  };
  reader.readAsDataURL(file);
});

// Form submit
$("gameForm").addEventListener("submit", async e => {
  e.preventDefault();
  const status = $("statusPicker").querySelector(".status-opt.active")?.dataset.val ?? "NotStarted";
  const input = {
    title:                 $("f_title").value.trim(),
    franchise:             $("f_franchise").value.trim() || null,
    sequence_in_franchise: $("f_sequence").value ? parseInt($("f_sequence").value) : null,
    release_date:          $("f_release_date").value || null,
    platform:              $("f_platform").value.trim(),
    status,
    progress_percent:      parseFloat($("f_progress").value),
    playtime_hours:        $("f_playtime").value ? parseFloat($("f_playtime").value) : null,
    rating:                state.formRating,
    notes:                 $("f_notes").value.trim() || null,
    cover_art_path:        $("f_cover_art_path").value || null,
    screenshots:           [],
    developer:             $("f_developer").value.trim() || null,
    publisher:             $("f_publisher").value.trim() || null,
    genres:                state.formGenres,
  };

  $("submitBtn").disabled = true;
  $("submitBtn").textContent = "Saving…";

  try {
    // Process cover image first (copy local file or download URL)
    if (input.cover_art_path) {
      try {
        const processedPath = await invoke("process_cover_image", { input: input.cover_art_path });
        input.cover_art_path = processedPath;
      } catch (imgErr) {
        console.error("Image processing failed:", imgErr);
        showToast("Warning: Could not process cover image", "error");
        // Continue saving without the cover art
        input.cover_art_path = null;
      }
    }

    if (state.editingGameId) {
      await invoke("update_game", { id: state.editingGameId, input });
      showToast("Game updated!", "success");
    } else {
      await invoke("add_game", { input });
      showToast("Game added!", "success");
    }
    closeModal();
    await refresh(true);
  } catch (err) {
    console.error("save failed:", err);
    showToast("Failed to save game", "error");
  } finally {
    $("submitBtn").disabled = false;
    $("submitBtn").textContent = state.editingGameId ? "Save Changes" : "Save Game";
  }
});


// =============================================================================
// Delete
// =============================================================================

function promptDelete(id) {
  const game = state.games.find(g => g.id === id);
  if (!game) return;
  state.pendingDeleteId = id;
  $("confirmGameTitle").textContent = game.title;
  $("confirmBackdrop").classList.remove("hidden");
}

$("confirmCancel").addEventListener("click", () => {
  $("confirmBackdrop").classList.add("hidden");
  state.pendingDeleteId = null;
});

$("confirmDelete").addEventListener("click", async () => {
  if (!state.pendingDeleteId) return;
  try {
    await invoke("delete_game", { id: state.pendingDeleteId });
    showToast("Game deleted", "info");
    $("confirmBackdrop").classList.add("hidden");
    state.pendingDeleteId = null;
    await refresh(true);
  } catch (e) {
    console.error("delete failed:", e);
    showToast("Failed to delete game", "error");
  }
});


// =============================================================================
// Navigation & View Switching
// =============================================================================

document.querySelectorAll(".nav-item").forEach(btn => {
  btn.addEventListener("click", () => {
    document.querySelectorAll(".nav-item").forEach(b => b.classList.remove("active"));
    btn.classList.add("active");
    state.activeView = btn.dataset.view;

    const isStats = state.activeView === "stats";
    const titlesMap = { library: "Library", backlog: "Backlog", wishlist: "Wishlist" };

    $("libraryView").classList.toggle("active", !isStats);
    $("statsView").classList.toggle("active",   isStats);
    $("sidebarFilters").style.display = isStats ? "none" : "";
    $("topbar").style.display         = isStats ? "none" : "";

    if (!isStats) {
      $("viewTitle").textContent = titlesMap[state.activeView];
      renderGames();
    } else {
      renderStats();
    }
  });
});


// =============================================================================
// Search & Sort
// =============================================================================

let searchTimeout;
$("searchInput").addEventListener("input", e => {
  clearTimeout(searchTimeout);
  searchTimeout = setTimeout(async () => {
    state.currentFilter.query = e.target.value.trim();
    await refresh();
  }, 250);
});

document.addEventListener("keydown", e => {
  if ((e.metaKey || e.ctrlKey) && e.key === "k") {
    e.preventDefault();
    $("searchInput").focus();
    $("searchInput").select();
  }
  if (e.key === "Escape") {
    closeDetail();
    closeModal();
    $("confirmBackdrop").classList.add("hidden");
  }
});

$("sortSelect").addEventListener("change", async e => {
  state.currentFilter.sortBy = e.target.value;
  await refresh();
});

$("statusFilters").addEventListener("click", e => {
  const chip = e.target.closest(".chip");
  if (!chip) return;
  state.currentFilter.status = chip.dataset.status;
  $("statusFilters").querySelectorAll(".chip").forEach(c => c.classList.toggle("active", c === chip));
  refresh();
});


// =============================================================================
// View toggle (grid / list)
// =============================================================================

$("gridViewBtn").addEventListener("click", () => {
  state.isListView = false;
  $("gridViewBtn").classList.add("active");
  $("listViewBtn").classList.remove("active");
  renderGames();
});
$("listViewBtn").addEventListener("click", () => {
  state.isListView = true;
  $("listViewBtn").classList.add("active");
  $("gridViewBtn").classList.remove("active");
  renderGames();
});


// =============================================================================
// Overlay & close buttons
// =============================================================================

$("overlay").addEventListener("click", closeDetail);
$("detailClose").addEventListener("click", closeDetail);
$("modalClose").addEventListener("click", closeModal);
$("cancelBtn").addEventListener("click", closeModal);
$("addGameBtn").addEventListener("click", () => openModal());


// =============================================================================
// Main refresh cycle
// =============================================================================

async function refresh(reloadMeta = false) {
  await loadGames();
  renderGames();
  if (reloadMeta) await loadMeta();
}


// =============================================================================
// Boot
// =============================================================================

async function init() {
  await Promise.all([loadGames(), loadMeta()]);
  renderGames();
  renderPlatformFilters();
  renderGenreFilters();
}

init();