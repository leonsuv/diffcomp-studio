// =============================================================================
// dc_app/app - Main Application Struct
// =============================================================================
// The core egui::App implementation for DiffComp Studio.
// =============================================================================

use crate::docking::{self, DiffCompTabViewer, Tab};
use crate::panels;
use crate::state::{AppState, ComputeMode, ToolMode};
use crate::undo::UndoCommand;
use dc_core::LoadConfig;
use dc_gpu::GpuDiffEngine;
use eframe::Frame;
use egui::{Context, Key};
use egui_dock::DockArea;
#[cfg(not(target_arch = "wasm32"))]
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
#[cfg(not(target_arch = "wasm32"))]
use std::process::Command;
use tracing::{error, info};

/// Messages for async operations (e.g. file loading on web)
enum AppMessage {
    DocumentsLoaded {
        project_id: u32,
        path: PathBuf,
        result: dc_core::CoreResult<Vec<(String, dc_core::RasterBuffer)>>,
    },
    AlignmentComputed {
        project_id: u32,
        layer_id: dc_core::LayerId,
        reference_id: dc_core::LayerId,
        reference: std::sync::Arc<dc_core::RasterBuffer>,
        target: std::sync::Arc<dc_core::RasterBuffer>,
        result: dc_core::CoreResult<dc_core::ForceFitResult>,
    },
    FileLoaded {
        project_id: u32,
        name: String,
        data: Vec<u8>,
    },
    SlipSheetFileLoaded {
        project_id: u32,
        name: String,
        data: Vec<u8>,
        req: crate::state::SlipSheetRequest,
    },
    Error(String),
    DiffComputed {
        project_id: u32,
        result: dc_core::RasterBuffer,
        generation: u64,
    },
    DiffFailed {
        project_id: u32,
        generation: u64,
        error: String,
    },
}

struct ProjectWorkspace {
    id: u32,
    name: String,
    state: AppState,
    textures: TextureCache,
    dock_state: egui_dock::DockState<Tab>,
    diff_generation: u64,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Serialize, Deserialize)]
struct InstanceMeta {
    instance_id: String,
    pid: u32,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Serialize, Deserialize)]
struct IpcMessage {
    kind: String,
    session_path: Option<String>,
    project_name: Option<String>,
    project_id: Option<u32>,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Serialize, Deserialize)]
struct DragOffer {
    offer_id: String,
    source_instance_id: String,
    project_id: u32,
    project_name: String,
    session_path: String,
    created_ms: u128,
    /// Set by the source when the mouse button is released, so targets measure
    /// freshness from the actual drop moment rather than from drag start.
    released_ms: Option<u128>,
}

/// The main DiffComp Studio application.
pub struct DiffCompApp {
    /// Current project identifier.
    current_project_id: u32,

    /// Current project display name.
    current_project_name: String,

    /// Application state
    state: AppState,

    /// Texture cache for rendered layers
    textures: TextureCache,

    /// Docking state
    dock_state: egui_dock::DockState<Tab>,

    /// Receiver for async messages
    rx: std::sync::mpsc::Receiver<AppMessage>,

    /// Sender for async messages (cloned for callbacks)
    tx: std::sync::mpsc::Sender<AppMessage>,

    /// Generation counter for diff computations.
    /// Incremented each time diff parameters change.
    /// Used to discard stale async results.
    diff_generation: u64,

    /// Cached egui context for requesting repaints from background threads.
    egui_ctx: Option<egui::Context>,

    /// Inactive project workspaces (the active one lives in top-level fields).
    other_projects: Vec<ProjectWorkspace>,

    /// Next unique project ID.
    next_project_id: u32,

    #[cfg(not(target_arch = "wasm32"))]
    instance_id: String,

    #[cfg(not(target_arch = "wasm32"))]
    ipc_inbox_dir: PathBuf,
    #[cfg(not(target_arch = "wasm32"))]
    last_ipc_poll: std::time::Instant,

    #[cfg(not(target_arch = "wasm32"))]
    active_drag_offer_id: Option<String>,

    #[cfg(not(target_arch = "wasm32"))]
    active_drag_project_id: Option<u32>,

    #[cfg(not(target_arch = "wasm32"))]
    pending_spawn_project_id: Option<u32>,

    #[cfg(not(target_arch = "wasm32"))]
    pending_spawn_due_ms: Option<u128>,
}

impl DiffCompApp {
    /// Create a new application instance.
    pub fn new(cc: &eframe::CreationContext<'_>, gpu_engine: Option<GpuDiffEngine>) -> Self {
        Self::new_with_startup_session(cc, gpu_engine, None)
    }

    /// Create a new application instance and optionally preload a session file.
    pub fn new_with_startup_session(
        cc: &eframe::CreationContext<'_>,
        gpu_engine: Option<GpuDiffEngine>,
        startup_session: Option<PathBuf>,
    ) -> Self {
        // Configure egui style
        configure_style(&cc.egui_ctx);

        info!("DiffComp Studio initialized");

        let (tx, rx) = std::sync::mpsc::channel();

        let mut app = Self {
            current_project_id: 0,
            current_project_name: "Project 1".to_string(),
            state: AppState::new(gpu_engine),
            textures: TextureCache::new(),
            dock_state: docking::default_layout(),
            rx,
            tx,
            diff_generation: 0,
            egui_ctx: None,
            other_projects: Vec::new(),
            next_project_id: 1,
            #[cfg(not(target_arch = "wasm32"))]
            instance_id: format!(
                "{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0)
            ),
            #[cfg(not(target_arch = "wasm32"))]
            last_ipc_poll: std::time::Instant::now(),
            #[cfg(not(target_arch = "wasm32"))]
            ipc_inbox_dir: std::env::temp_dir(),
            #[cfg(not(target_arch = "wasm32"))]
            active_drag_offer_id: None,
            #[cfg(not(target_arch = "wasm32"))]
            active_drag_project_id: None,
            #[cfg(not(target_arch = "wasm32"))]
            pending_spawn_project_id: None,
            #[cfg(not(target_arch = "wasm32"))]
            pending_spawn_due_ms: None,
        };

        #[cfg(not(target_arch = "wasm32"))]
        app.init_ipc_dirs();

        if let Some(path) = startup_session {
            app.load_session_from_path(&path);
        }

        app
    }

    fn take_current_workspace(&mut self) -> ProjectWorkspace {
        ProjectWorkspace {
            id: self.current_project_id,
            name: std::mem::take(&mut self.current_project_name),
            state: std::mem::take(&mut self.state),
            textures: std::mem::take(&mut self.textures),
            dock_state: std::mem::replace(&mut self.dock_state, docking::default_layout()),
            diff_generation: self.diff_generation,
        }
    }

    fn set_current_workspace(&mut self, workspace: ProjectWorkspace) {
        self.current_project_id = workspace.id;
        self.current_project_name = workspace.name;
        self.state = workspace.state;
        self.textures = workspace.textures;
        self.dock_state = workspace.dock_state;
        self.diff_generation = workspace.diff_generation;
    }

    fn switch_to_project(&mut self, project_id: u32) {
        if self.current_project_id == project_id {
            return;
        }

        if let Some(idx) = self.other_projects.iter().position(|p| p.id == project_id) {
            let target = self.other_projects.remove(idx);
            let current = self.take_current_workspace();
            self.set_current_workspace(target);
            self.other_projects.push(current);
        }
    }

    fn create_project_tab(&mut self) {
        let id = self.next_project_id;
        self.next_project_id += 1;
        let name = format!("Project {}", id + 1);
        let gpu_engine = self.state.gpu_engine.clone();

        let current = self.take_current_workspace();
        self.other_projects.push(current);

        let workspace = ProjectWorkspace {
            id,
            name,
            state: AppState::new_with_gpu_arc(gpu_engine),
            textures: TextureCache::new(),
            dock_state: docking::default_layout(),
            diff_generation: 0,
        };
        self.set_current_workspace(workspace);
    }

    fn close_project_tab(&mut self, project_id: u32) {
        if self.current_project_id == project_id {
            if self.other_projects.is_empty() {
                return;
            }
            let next = self.other_projects.remove(0);
            self.set_current_workspace(next);
            return;
        }

        if let Some(idx) = self.other_projects.iter().position(|p| p.id == project_id) {
            self.other_projects.remove(idx);
        }
    }

    fn project_tabs(&self) -> Vec<(u32, String, bool)> {
        let mut tabs = Vec::with_capacity(self.other_projects.len() + 1);
        tabs.push((
            self.current_project_id,
            self.current_project_name.clone(),
            true,
        ));
        for project in &self.other_projects {
            tabs.push((project.id, project.name.clone(), false));
        }
        // Sort by ID to maintain stable insertion order (IDs increment at creation)
        // This keeps tabs in consistent position even when switching between them
        tabs.sort_by_key(|(id, _, _)| *id);
        tabs
    }

    /// Navigate to the next project tab (by position, not by ID).
    fn next_project_tab(&mut self) {
        let tabs = self.project_tabs();
        let current_pos = tabs.iter().position(|(_, _, active)| *active).unwrap_or(0);
        let next_pos = (current_pos + 1) % tabs.len();
        if let Some((id, _, _)) = tabs.get(next_pos) {
            self.switch_to_project(*id);
        }
    }

    /// Navigate to the previous project tab (by position, not by ID).
    fn prev_project_tab(&mut self) {
        let tabs = self.project_tabs();
        let current_pos = tabs.iter().position(|(_, _, active)| *active).unwrap_or(0);
        let next_pos = if current_pos == 0 {
            tabs.len().saturating_sub(1)
        } else {
            current_pos - 1
        };
        if let Some((id, _, _)) = tabs.get(next_pos) {
            self.switch_to_project(*id);
        }
    }

    /// Switch to the nth project tab by position (1-indexed).
    /// Does nothing if position is out of bounds.
    fn switch_to_project_by_index(&mut self, index: usize) {
        let tabs = self.project_tabs();
        if let Some((id, _, _)) = tabs.get(index) {
            self.switch_to_project(*id);
        }
    }

    /// Swap two projects by their position in the tab list.
    fn swap_projects_by_position(&mut self, pos1: usize, pos2: usize) {
        let tabs = self.project_tabs();
        if pos1 >= tabs.len() || pos2 >= tabs.len() || pos1 == pos2 {
            return;
        }

        let (id1, _, active1) = tabs[pos1];
        let (id2, _, active2) = tabs[pos2];

        if id1 == self.current_project_id && !active1 {
            // This shouldn't happen, but handle it
            return;
        }

        // Both are in other_projects
        if !active1 && !active2 {
            let idx1 = self
                .other_projects
                .iter()
                .position(|p| p.id == id1)
                .unwrap();
            let idx2 = self
                .other_projects
                .iter()
                .position(|p| p.id == id2)
                .unwrap();
            self.other_projects.swap(idx1, idx2);
        }
        // id1 is current, id2 is in other_projects
        else if active1 && !active2 {
            let idx2 = self
                .other_projects
                .iter()
                .position(|p| p.id == id2)
                .unwrap();
            let workspace2 = self.other_projects.remove(idx2);
            let current = self.take_current_workspace();
            self.other_projects.insert(idx2, current);
            self.set_current_workspace(workspace2);
        }
        // id2 is current, id1 is in other_projects
        else if !active1 && active2 {
            let idx1 = self
                .other_projects
                .iter()
                .position(|p| p.id == id1)
                .unwrap();
            let workspace1 = self.other_projects.remove(idx1);
            let current = self.take_current_workspace();
            self.other_projects.insert(idx1, current);
            self.set_current_workspace(workspace1);
        }
    }

    /// Launch a separate app process with the selected project session.
    #[cfg(not(target_arch = "wasm32"))]
    fn open_project_in_new_instance(&mut self, project_id: u32) {
        let (session, project_name) = if project_id == self.current_project_id {
            (&self.state.session, self.current_project_name.clone())
        } else if let Some(project) = self.other_projects.iter().find(|p| p.id == project_id) {
            (&project.state.session, project.name.clone())
        } else {
            self.state.ui.set_status("Project not found");
            return;
        };

        let mut path = std::env::temp_dir();
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        path.push(format!("diffcomp-project-{}-{}.dcs", project_id, ts));

        if let Err(e) = crate::persistence::save_session(session, &path) {
            self.state
                .ui
                .set_status(format!("Failed to export project: {}", e));
            return;
        }

        match std::env::current_exe() {
            Ok(exe) => {
                if let Err(e) = Command::new(exe).arg("--session").arg(&path).spawn() {
                    self.state
                        .ui
                        .set_status(format!("Failed to open new instance: {}", e));
                    return;
                }
                self.close_project_tab(project_id);
                self.state
                    .ui
                    .set_status(format!("Moved '{}' to new app instance", project_name));
            }
            Err(e) => {
                self.state
                    .ui
                    .set_status(format!("Failed to locate executable: {}", e));
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn ipc_root_dir() -> PathBuf {
        std::env::temp_dir().join("diffcomp-studio-ipc")
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn ipc_instances_dir() -> PathBuf {
        Self::ipc_root_dir().join("instances")
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn ipc_inboxes_dir() -> PathBuf {
        Self::ipc_root_dir().join("inboxes")
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn ipc_offers_dir() -> PathBuf {
        Self::ipc_root_dir().join("offers")
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn init_ipc_dirs(&mut self) {
        let instances_dir = Self::ipc_instances_dir();
        let inboxes_dir = Self::ipc_inboxes_dir();
        let offers_dir = Self::ipc_offers_dir();
        let _ = std::fs::create_dir_all(&instances_dir);
        let _ = std::fs::create_dir_all(&inboxes_dir);
        let _ = std::fs::create_dir_all(&offers_dir);

        self.ipc_inbox_dir = inboxes_dir.join(&self.instance_id);
        let _ = std::fs::create_dir_all(&self.ipc_inbox_dir);

        let meta = InstanceMeta {
            instance_id: self.instance_id.clone(),
            pid: std::process::id(),
        };
        let meta_path = instances_dir.join(format!("{}.json", self.instance_id));
        if let Ok(payload) = serde_json::to_vec(&meta) {
            let _ = std::fs::write(meta_path, payload);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn list_other_instances(&self) -> Vec<InstanceMeta> {
        let mut out = Vec::new();
        let dir = Self::ipc_instances_dir();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) != Some("json") {
                    continue;
                }
                if let Ok(bytes) = std::fs::read(&path) {
                    if let Ok(meta) = serde_json::from_slice::<InstanceMeta>(&bytes) {
                        if meta.instance_id != self.instance_id {
                            out.push(meta);
                        }
                    }
                }
            }
        }
        out
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn create_project_tab_from_session(
        &mut self,
        name: String,
        session: crate::state::SessionState,
    ) {
        let id = self.next_project_id;
        self.next_project_id += 1;
        let gpu_engine = self.state.gpu_engine.clone();

        let current = self.take_current_workspace();
        self.other_projects.push(current);

        let mut state = AppState::new_with_gpu_arc(gpu_engine);
        state.session = session;
        let workspace = ProjectWorkspace {
            id,
            name,
            state,
            textures: TextureCache::new(),
            dock_state: docking::default_layout(),
            diff_generation: 0,
        };
        self.set_current_workspace(workspace);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn create_drag_offer_for_project(&mut self, project_id: u32) {
        let (session, project_name) = if project_id == self.current_project_id {
            (&self.state.session, self.current_project_name.clone())
        } else if let Some(project) = self.other_projects.iter().find(|p| p.id == project_id) {
            (&project.state.session, project.name.clone())
        } else {
            return;
        };

        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let offer_id = format!("{}-{}-{}", self.instance_id, project_id, ts);

        let transfers_dir = Self::ipc_root_dir().join("transfers");
        let _ = std::fs::create_dir_all(&transfers_dir);
        let session_path = transfers_dir.join(format!("offer-{}.dcs", offer_id));
        if crate::persistence::save_session(session, &session_path).is_err() {
            return;
        }

        let offer = DragOffer {
            offer_id: offer_id.clone(),
            source_instance_id: self.instance_id.clone(),
            project_id,
            project_name,
            session_path: session_path.to_string_lossy().into_owned(),
            created_ms: ts,
            released_ms: None,
        };

        let offer_path = Self::ipc_offers_dir().join(format!("{}.json", offer_id));
        if let Ok(bytes) = serde_json::to_vec(&offer) {
            let _ = std::fs::write(offer_path, bytes);
            self.active_drag_offer_id = Some(offer.offer_id);
            self.active_drag_project_id = Some(project_id);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn latest_external_drag_offer(&self) -> Option<DragOffer> {
        let mut best: Option<DragOffer> = None;
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);

        let Ok(entries) = std::fs::read_dir(Self::ipc_offers_dir()) else {
            return None;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            let Ok(offer) = serde_json::from_slice::<DragOffer>(&bytes) else {
                continue;
            };
            if offer.source_instance_id == self.instance_id {
                continue;
            }
            // Expire stale offers after 30 seconds.
            if now_ms.saturating_sub(offer.created_ms) > 30_000 {
                continue;
            }
            match &best {
                Some(curr) if curr.created_ms >= offer.created_ms => {}
                _ => best = Some(offer),
            }
        }
        best
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn maybe_accept_drag_offer(&mut self, ctx: &Context, _drop_rect: egui::Rect) {
        let pointer = ctx.input(|i| i.pointer.clone());
        // Don't accept while the mouse button is still held.
        if pointer.primary_down() {
            return;
        }
        // Require the pointer to be present in this window.  We try both
        // interact_pos (set during/after a drag) and hover_pos (set during
        // normal hovering) because macOS mouse-capture means the target window
        // may only receive CursorMoved after the button has been released.
        if pointer.interact_pos().is_none() && pointer.hover_pos().is_none() {
            return;
        }

        let Some(offer) = self.latest_external_drag_offer() else {
            return;
        };

        // Measure freshness from released_ms (written at drag-end by the
        // source) so long drags don't falsely expire.  Fall back to
        // created_ms with a generous window for offers without the field.
        let now = Self::now_ms();
        let too_old = match offer.released_ms {
            Some(released) => now.saturating_sub(released) > 3_000,
            None => now.saturating_sub(offer.created_ms) > 10_000,
        };
        if too_old {
            return;
        }

        let session_path = PathBuf::from(&offer.session_path);
        match crate::persistence::load_session(&session_path) {
            Ok(session) => {
                self.create_project_tab_from_session(offer.project_name.clone(), session);

                let remove_msg = IpcMessage {
                    kind: "remove_project".to_string(),
                    session_path: None,
                    project_name: None,
                    project_id: Some(offer.project_id),
                };
                let source_inbox = Self::ipc_inboxes_dir().join(&offer.source_instance_id);
                let _ = std::fs::create_dir_all(&source_inbox);
                let ack_name = format!("remove-{}.json", offer.offer_id);
                if let Ok(bytes) = serde_json::to_vec(&remove_msg) {
                    let _ = std::fs::write(source_inbox.join(ack_name), bytes);
                }

                let _ = std::fs::remove_file(
                    Self::ipc_offers_dir().join(format!("{}.json", offer.offer_id)),
                );
                let _ = std::fs::remove_file(session_path);
                self.state.ui.set_status(format!(
                    "Dropped '{}' from another instance",
                    offer.project_name
                ));
            }
            Err(e) => {
                self.state
                    .ui
                    .set_status(format!("Failed to import dropped project: {}", e));
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn clear_active_drag_offer(&mut self) {
        if let Some(offer_id) = self.active_drag_offer_id.take() {
            let _ = std::fs::remove_file(Self::ipc_offers_dir().join(format!("{}.json", offer_id)));
        }
        self.active_drag_project_id = None;
        self.pending_spawn_project_id = None;
        self.pending_spawn_due_ms = None;
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn now_ms() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn maybe_flush_pending_spawn(&mut self) {
        let (Some(project_id), Some(due_ms)) =
            (self.pending_spawn_project_id, self.pending_spawn_due_ms)
        else {
            return;
        };

        if Self::now_ms() < due_ms {
            return;
        }

        // Fallback path: no target instance consumed the drag offer in time.
        self.open_project_in_new_instance(project_id);
        self.clear_active_drag_offer();
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn maybe_spawn_instance_from_drag_release(
        &mut self,
        ctx: &Context,
        drop_rect: egui::Rect,
        had_reorder_drop: bool,
    ) {
        let pointer = ctx.input(|i| i.pointer.clone());
        if !pointer.any_released() {
            return;
        }

        if had_reorder_drop {
            self.clear_active_drag_offer();
            return;
        }

        let Some(pos) = pointer.interact_pos() else {
            return;
        };

        // Released on own tab row: cancel cross-instance offer.
        if drop_rect.contains(pos) {
            self.clear_active_drag_offer();
            return;
        }

        // Released outside own tab row: schedule fallback spawn and stamp the
        // offer with the release time so the target can measure freshness from
        // this moment rather than from when the drag started.
        if let Some(project_id) = self.active_drag_project_id {
            let now = Self::now_ms();
            // Refresh the offer file with released_ms so long drags don't fail
            // the target's freshness check.
            if let Some(offer_id) = &self.active_drag_offer_id {
                let offer_path = Self::ipc_offers_dir().join(format!("{}.json", offer_id));
                if let Ok(bytes) = std::fs::read(&offer_path) {
                    if let Ok(mut offer) = serde_json::from_slice::<DragOffer>(&bytes) {
                        offer.released_ms = Some(now);
                        if let Ok(updated) = serde_json::to_vec(&offer) {
                            let _ = std::fs::write(&offer_path, updated);
                        }
                    }
                }
            }
            self.pending_spawn_project_id = Some(project_id);
            self.pending_spawn_due_ms = Some(now + 1_800);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn send_project_to_instance(&mut self, project_id: u32, target_instance_id: &str) {
        let (session, project_name) = if project_id == self.current_project_id {
            (&self.state.session, self.current_project_name.clone())
        } else if let Some(project) = self.other_projects.iter().find(|p| p.id == project_id) {
            (&project.state.session, project.name.clone())
        } else {
            self.state.ui.set_status("Project not found");
            return;
        };

        let transfers_dir = Self::ipc_root_dir().join("transfers");
        let _ = std::fs::create_dir_all(&transfers_dir);
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let session_path =
            transfers_dir.join(format!("{}-{}-{}.dcs", target_instance_id, project_id, ts));
        if let Err(e) = crate::persistence::save_session(session, &session_path) {
            self.state
                .ui
                .set_status(format!("Failed to export project: {}", e));
            return;
        }

        let inbox = Self::ipc_inboxes_dir().join(target_instance_id);
        if std::fs::create_dir_all(&inbox).is_err() {
            self.state
                .ui
                .set_status("Target instance inbox unavailable");
            return;
        }

        let msg = IpcMessage {
            kind: "import_session".to_string(),
            session_path: Some(session_path.to_string_lossy().into_owned()),
            project_name: Some(project_name.clone()),
            project_id: None,
        };
        let msg_path = inbox.join(format!("msg-{}-{}.json", project_id, ts));
        if let Ok(bytes) = serde_json::to_vec(&msg) {
            if let Err(e) = std::fs::write(&msg_path, bytes) {
                self.state
                    .ui
                    .set_status(format!("Failed to send project: {}", e));
                return;
            }
        } else {
            self.state
                .ui
                .set_status("Failed to serialize transfer message");
            return;
        }

        self.close_project_tab(project_id);
        self.state.ui.set_status(format!(
            "Moved '{}' to instance {}",
            project_name, target_instance_id
        ));
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn process_incoming_ipc_messages(&mut self) {
        let Ok(entries) = std::fs::read_dir(&self.ipc_inbox_dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            let Ok(msg) = serde_json::from_slice::<IpcMessage>(&bytes) else {
                let _ = std::fs::remove_file(&path);
                continue;
            };

            if msg.kind == "import_session" {
                let Some(session_path_str) = msg.session_path.clone() else {
                    let _ = std::fs::remove_file(&path);
                    continue;
                };
                let session_path = PathBuf::from(&session_path_str);
                match crate::persistence::load_session(&session_path) {
                    Ok(session) => {
                        self.create_project_tab_from_session(
                            msg.project_name
                                .clone()
                                .unwrap_or_else(|| "Imported Project".to_string()),
                            session,
                        );
                        self.state
                            .ui
                            .set_status("Received project from another instance".to_string());
                    }
                    Err(e) => {
                        self.state
                            .ui
                            .set_status(format!("Failed to import project: {}", e));
                    }
                }
                let _ = std::fs::remove_file(&session_path);
            } else if msg.kind == "remove_project" {
                if let Some(project_id) = msg.project_id {
                    self.close_project_tab(project_id);
                    self.clear_active_drag_offer();
                    self.state
                        .ui
                        .set_status("Project moved to another instance");
                }
            }

            let _ = std::fs::remove_file(&path);
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn process_incoming_ipc_messages(&mut self) {}

    #[cfg(target_arch = "wasm32")]
    fn open_project_in_new_instance(&mut self, _project_id: u32) {
        self.state
            .ui
            .set_status("Opening a new app instance is not supported in web mode");
    }

    /// Handle keyboard shortcuts.
    fn handle_shortcuts(&mut self, ctx: &Context) {
        let input = ctx.input(|i| i.clone());

        // Don't process non-modifier shortcuts when a text field, popup, or modal has focus.
        // This prevents typing "281" in an offset field from toggling layers 2 and 1,
        // or Backspace in a text field from deleting a layer.
        let text_has_focus = ctx.memory(|mem| mem.focused().is_some());

        // Ctrl+O: Open file (always active — modifier shortcuts are safe)
        if input.modifiers.command && input.key_pressed(Key::O) {
            self.open_file_dialog(ctx);
        }

        // Ctrl+S: Save session
        if input.modifiers.command && input.key_pressed(Key::S) {
            self.save_session();
        }

        // Ctrl+Z: Undo
        if input.modifiers.command && !input.modifiers.shift && input.key_pressed(Key::Z) {
            self.perform_undo();
        }

        // Ctrl+Shift+Z: Redo
        if input.modifiers.command && input.modifiers.shift && input.key_pressed(Key::Z) {
            self.perform_redo();
        }

        // Ctrl+Shift+Right: Next project tab
        if input.modifiers.command && input.modifiers.shift && input.key_pressed(Key::ArrowRight) {
            self.next_project_tab();
        }

        // Ctrl+Shift+Left: Previous project tab
        if input.modifiers.command && input.modifiers.shift && input.key_pressed(Key::ArrowLeft) {
            self.prev_project_tab();
        }

        // Ctrl+Shift+1-9: Jump to nth project tab (0-indexed, so Ctrl+Shift+1 is first tab)
        if input.modifiers.command && input.modifiers.shift {
            let num_keys = [
                Key::Num1,
                Key::Num2,
                Key::Num3,
                Key::Num4,
                Key::Num5,
                Key::Num6,
                Key::Num7,
                Key::Num8,
                Key::Num9,
            ];
            for (index, key) in num_keys.iter().enumerate() {
                if input.key_pressed(*key) {
                    self.switch_to_project_by_index(index);
                    break;
                }
            }
        }

        // --- All shortcuts below here are blocked when a text widget has focus ---
        if text_has_focus {
            return;
        }

        // Delete: Remove selected annotation OR selected layer
        if input.key_pressed(Key::Delete) || input.key_pressed(Key::Backspace) {
            let mut deleted_annot = false;
            if let Some((layer_id, annotation_id)) = self.state.session.selected_annotation.clone()
            {
                if let Some(layer) = self.state.session.get_layer_mut(layer_id) {
                    // Find the annotation and its index before removing
                    if let Some(idx) = layer.annotations.iter().position(|a| a.id == annotation_id)
                    {
                        let removed = layer.annotations.remove(idx);
                        self.state
                            .session
                            .undo_stack
                            .push(UndoCommand::RemoveAnnotation {
                                layer_id,
                                annotation: Box::new(removed),
                                index: idx,
                            });
                    }
                    self.state.session.is_dirty = true;
                    self.state
                        .ui
                        .set_status(crate::i18n::tr("status.annotation_deleted"));
                    deleted_annot = true;
                }
                // Clear selection after deletion
                self.state.session.selected_annotation = None;
            }

            // Only delete layer if we didn't just delete an annotation
            if !deleted_annot {
                if let Some(id) = self.state.session.selected_layer {
                    // Snapshot the layer before removing for undo
                    if let Some(idx) = self.state.session.layers.iter().position(|l| l.id == id) {
                        let layer_snapshot = self.state.session.layers[idx].clone();
                        let was_reference = layer_snapshot.is_reference;
                        let old_selected = self.state.session.selected_layer;
                        self.state.session.remove_layer(id);
                        self.state
                            .session
                            .undo_stack
                            .push(UndoCommand::RemoveLayer {
                                layer_id: id,
                                index: idx,
                                layer: Box::new(layer_snapshot),
                                was_reference,
                                old_selected,
                            });
                        self.state
                            .ui
                            .set_status(crate::i18n::tr("status.layer_deleted"));
                    }
                }
            }
        }

        // Space: Toggle pan mode
        if input.key_pressed(Key::Space) {
            self.state.ui.tool_mode = ToolMode::Pan;
            self.state.tools.active_tool = None;
        }

        // V: Select Mode
        if !input.modifiers.command && input.key_pressed(Key::H) {
            self.state.ui.tool_mode = ToolMode::Pan;
            self.state.tools.active_tool = None;
        }
        if !input.modifiers.command && input.key_pressed(Key::Z) {
            self.state.ui.tool_mode = ToolMode::Zoom;
            self.state.tools.active_tool = None;
        }
        if input.key_pressed(Key::V) {
            self.state.ui.tool_mode = ToolMode::Select;
            self.state.tools.active_tool = None;
            self.state
                .ui
                .set_status(crate::i18n::tr("status.select_mode"));
        }

        // Esc: Cancel Selection / Go to Pan
        if input.key_pressed(Key::Escape) {
            if self.state.session.selected_annotation.is_some() {
                self.state.session.selected_annotation = None;
                self.state
                    .ui
                    .set_status(crate::i18n::tr("status.selection_cleared"));
            } else if self.state.tools.active_tool.is_some() {
                self.state.tools.active_tool = None;
                self.state.ui.tool_mode = ToolMode::Pan;
                self.state
                    .ui
                    .set_status(crate::i18n::tr("status.tool_cancelled"));
            } else {
                self.state.ui.tool_mode = ToolMode::Pan;
            }
        }

        // F: Fit to window
        if input.key_pressed(Key::F) {
            if self.state.session.reference_layer().is_some() {
                self.state.ui.fit_view_requested = true;
            }
        }

        // R: Reset viewport
        if input.key_pressed(Key::R) {
            self.state.session.viewport.reset();
        }

        // +/=: Zoom in
        if input.key_pressed(Key::Plus) || input.key_pressed(Key::Equals) {
            self.state.session.viewport.apply_zoom(1.25);
        }

        // -: Zoom out
        if input.key_pressed(Key::Minus) {
            self.state.session.viewport.apply_zoom(0.8);
        }

        // 0: Zoom to 100%
        if input.key_pressed(Key::Num0) {
            self.state.session.viewport.zoom = 1.0;
        }

        // 1-9: Toggle layer visibility (only when Ctrl is not pressed to avoid conflict with Num0)
        if !input.modifiers.command {
            for (i, key) in [
                Key::Num1,
                Key::Num2,
                Key::Num3,
                Key::Num4,
                Key::Num5,
                Key::Num6,
                Key::Num7,
                Key::Num8,
                Key::Num9,
            ]
            .iter()
            .enumerate()
            {
                if input.key_pressed(*key) {
                    if let Some(layer) = self.state.session.layers.get_mut(i) {
                        let old_visible = layer.visible;
                        layer.visible = !layer.visible;
                        self.state
                            .session
                            .undo_stack
                            .push(UndoCommand::SetLayerVisibility {
                                layer_id: layer.id,
                                old_visible,
                                new_visible: layer.visible,
                            });
                    }
                }
            }
        }
    }

    /// Perform undo: pop the last command and apply its reverse.
    fn perform_undo(&mut self) {
        if let Some(cmd) = self.state.session.undo_stack.pop_undo() {
            let desc = cmd.description();
            let redo_cmd = cmd.undo(&mut self.state.session);
            self.state.session.undo_stack.push_redo(redo_cmd);
            self.state.ui.set_status(format!("Undo: {}", desc));
            // Invalidate diff so it recomputes
            self.state.ui.diff_invalidated = true;
            // Invalidate texture cache for any changed layers
            self.textures.clear();
        } else {
            self.state
                .ui
                .set_status(crate::i18n::tr("status.nothing_to_undo"));
        }
    }

    /// Perform redo: pop the last undone command and re-apply it.
    fn perform_redo(&mut self) {
        if let Some(cmd) = self.state.session.undo_stack.pop_redo() {
            let desc = cmd.description();
            let undo_cmd = cmd.redo(&mut self.state.session);
            self.state.session.undo_stack.push_for_redo(undo_cmd);
            self.state.ui.set_status(format!("Redo: {}", desc));
            // Invalidate diff so it recomputes
            self.state.ui.diff_invalidated = true;
            // Invalidate texture cache for any changed layers
            self.textures.clear();
        } else {
            self.state
                .ui
                .set_status(crate::i18n::tr("status.nothing_to_redo"));
        }
    }

    /// Check for async messages.
    fn check_messages(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                AppMessage::DocumentsLoaded {
                    project_id,
                    path,
                    result,
                } => {
                    let previous = self.current_project_id;
                    if project_id != previous
                        && !self.other_projects.iter().any(|p| p.id == project_id)
                    {
                        continue;
                    }
                    self.switch_to_project(project_id);
                    self.state.ui.pending_operations =
                        self.state.ui.pending_operations.saturating_sub(1);
                    match result {
                        Ok(documents) => {
                            for (name, buffer) in documents {
                                match self.state.session.add_layer(name, path.clone(), buffer) {
                                    Ok(id) => {
                                        let index = self
                                            .state
                                            .session
                                            .layers
                                            .iter()
                                            .position(|l| l.id == id)
                                            .unwrap_or(0);
                                        let layer =
                                            self.state.session.get_layer(id).unwrap().clone();
                                        self.state.session.undo_stack.push(UndoCommand::AddLayer {
                                            layer_id: id,
                                            index,
                                            layer: Box::new(layer),
                                        });
                                        if self.state.session.layers.len() == 1 {
                                            self.state.ui.fit_view_requested = true;
                                        }
                                        self.align_layer(id);
                                    }
                                    Err(e) => {
                                        self.state
                                            .ui
                                            .set_status(format!("Failed to add document: {e}"));
                                        break;
                                    }
                                }
                            }
                            self.state.ui.diff_invalidated = true;
                        }
                        Err(e) => self
                            .state
                            .ui
                            .set_status(format!("Failed to load {}: {e}", path.display())),
                    }
                    self.state.ui.is_loading = self.state.ui.pending_operations > 0
                        || self.state.ui.diff_running.is_some();
                    self.switch_to_project(previous);
                }
                AppMessage::AlignmentComputed {
                    project_id,
                    layer_id,
                    reference_id,
                    reference,
                    target,
                    result,
                } => {
                    let previous = self.current_project_id;
                    if project_id != previous
                        && !self.other_projects.iter().any(|p| p.id == project_id)
                    {
                        continue;
                    }
                    self.switch_to_project(project_id);
                    self.state.ui.pending_operations =
                        self.state.ui.pending_operations.saturating_sub(1);
                    let valid = self.state.session.reference_layer().is_some_and(|r| {
                        r.id == reference_id && std::sync::Arc::ptr_eq(&r.original, &reference)
                    }) && self
                        .state
                        .session
                        .get_layer(layer_id)
                        .is_some_and(|l| std::sync::Arc::ptr_eq(&l.original, &target));
                    if valid {
                        self.apply_alignment_result(layer_id, reference_id, result);
                    }
                    self.state.ui.is_loading = self.state.ui.pending_operations > 0
                        || self.state.ui.diff_running.is_some();
                    self.switch_to_project(previous);
                }
                AppMessage::FileLoaded {
                    project_id,
                    name,
                    data,
                } => {
                    if project_id != self.current_project_id {
                        continue;
                    }
                    info!(name = %name, size = data.len(), "Async file loaded");
                    let config = LoadConfig::default();
                    match self
                        .state
                        .loader_registry
                        .load_from_memory(&data, &name, &config)
                    {
                        Ok(buffer) => {
                            let path = PathBuf::from(&name);
                            match self.state.session.add_layer(name.clone(), path, buffer) {
                                Ok(id) => {
                                    self.state.ui.set_status(format!("Loaded {}", name));
                                    // Record undo for add layer
                                    if let Some(layer) = self.state.session.get_layer(id) {
                                        let idx = self
                                            .state
                                            .session
                                            .layers
                                            .iter()
                                            .position(|l| l.id == id)
                                            .unwrap_or(0);
                                        self.state.session.undo_stack.push(UndoCommand::AddLayer {
                                            layer_id: id,
                                            index: idx,
                                            layer: Box::new(layer.clone()),
                                        });
                                    }
                                    self.align_layer(id);
                                }
                                Err(e) => {
                                    error!("Failed to add layer: {}", e);
                                    self.state.ui.set_status(format!("Error: {}", e));
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to load file: {}", e);
                            self.state.ui.set_status(format!("Failed to load: {}", e));
                        }
                    }
                }
                AppMessage::SlipSheetFileLoaded {
                    project_id,
                    name,
                    data,
                    req,
                } => {
                    if project_id != self.current_project_id {
                        continue;
                    }
                    self.perform_slipsheet_memory(data, &name, req);
                }
                AppMessage::Error(e) => {
                    error!("Async error: {}", e);
                    self.state.ui.set_status(format!("Error: {}", e));
                    self.state.ui.is_loading = false;
                }
                AppMessage::DiffComputed {
                    project_id,
                    result,
                    generation,
                } => {
                    let (state, textures, current_generation) =
                        if project_id == self.current_project_id {
                            (&mut self.state, &mut self.textures, self.diff_generation)
                        } else if let Some(project) =
                            self.other_projects.iter_mut().find(|p| p.id == project_id)
                        {
                            (
                                &mut project.state,
                                &mut project.textures,
                                project.diff_generation,
                            )
                        } else {
                            continue;
                        };
                    if state.ui.diff_running == Some(generation) {
                        state.ui.diff_running = None;
                        state.ui.is_loading = state.ui.pending_operations > 0;
                    }
                    if generation == current_generation && !state.ui.diff_invalidated {
                        state.session.diff_result = Some(result);
                        state.ui.diff_failed = false;
                        state.ui.set_status(crate::i18n::tr("status.diff_complete"));
                        textures.remove(u64::MAX);
                    }
                }
                AppMessage::DiffFailed {
                    project_id,
                    generation,
                    error: e,
                } => {
                    let (state, current_generation) = if project_id == self.current_project_id {
                        (&mut self.state, self.diff_generation)
                    } else if let Some(project) =
                        self.other_projects.iter_mut().find(|p| p.id == project_id)
                    {
                        (&mut project.state, project.diff_generation)
                    } else {
                        continue;
                    };
                    if state.ui.diff_running == Some(generation) {
                        state.ui.diff_running = None;
                        state.ui.is_loading = state.ui.pending_operations > 0;
                    }
                    if generation == current_generation {
                        state.ui.diff_failed = true;
                        state.ui.set_status(format!("Diff failed: {e}"));
                    }
                }
            }
        }
    }

    /// Open the file dialog to load a document.
    fn open_file_dialog(&mut self, _ctx: &Context) {
        // Build filter for rfd
        #[cfg(not(target_arch = "wasm32"))]
        {
            let extensions = self.state.loader_registry.supported_extensions();
            let filter: Vec<&str> = extensions.iter().copied().collect();

            if let Some(paths) = rfd::FileDialog::new()
                .add_filter("Supported Documents", &filter)
                .add_filter("PDF Documents", &["pdf"])
                .add_filter("Images", &["png", "jpg", "jpeg", "tiff", "bmp"])
                .pick_files()
            {
                for path in paths {
                    self.load_file(path);
                }
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::closure::Closure;
            use wasm_bindgen::JsCast;

            let document = web_sys::window().unwrap().document().unwrap();
            let input = document
                .create_element("input")
                .unwrap()
                .dyn_into::<web_sys::HtmlInputElement>()
                .unwrap();
            input.set_attribute("type", "file").unwrap();
            input.set_attribute("style", "display:none").unwrap();
            // Accept images and PDF
            input
                .set_attribute("accept", ".png,.jpg,.jpeg,.tiff,.bmp,.webp,.pdf")
                .unwrap();

            let tx = self.tx.clone();
            let ctx = _ctx.clone();
            let project_id = self.current_project_id;

            let closure = Closure::wrap(Box::new(move |event: web_sys::Event| {
                let input: web_sys::HtmlInputElement = event.target().unwrap().dyn_into().unwrap();
                if let Some(files) = input.files() {
                    if let Some(file) = files.get(0) {
                        let name = file.name();
                        let tx = tx.clone();
                        let ctx = ctx.clone();

                        let reader = web_sys::FileReader::new().unwrap();
                        let reader_clone = reader.clone();

                        let onload = Closure::wrap(Box::new(move |_e: web_sys::Event| {
                            let result = reader_clone.result().unwrap();
                            let array_buffer = result.dyn_into::<js_sys::ArrayBuffer>().unwrap();
                            let uint8_array = js_sys::Uint8Array::new(&array_buffer);
                            let mut data = vec![0; uint8_array.length() as usize];
                            uint8_array.copy_to(&mut data);

                            let _ = tx.send(AppMessage::FileLoaded {
                                project_id,
                                name: name.clone(),
                                data,
                            });
                            ctx.request_repaint();
                        }) as Box<dyn FnMut(_)>);

                        reader.set_onload(Some(onload.as_ref().unchecked_ref()));
                        reader.read_as_array_buffer(&file).unwrap();
                        onload.forget();
                    }
                }
            }) as Box<dyn FnMut(_)>);

            input
                .add_event_listener_with_callback("change", closure.as_ref().unchecked_ref())
                .unwrap();
            closure.forget(); // Leak listener to keep it alive for the event

            document.body().unwrap().append_child(&input).unwrap();
            input.click();
        }
    }

    /// Open dialog for slip-sheeting
    fn open_slipsheet_dialog(&mut self, _ctx: &Context, req: crate::state::SlipSheetRequest) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let extensions = self.state.loader_registry.supported_extensions();
            let filter: Vec<&str> = extensions.iter().copied().collect();

            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Supported Documents", &filter)
                .add_filter("PDF Documents", &["pdf"])
                .add_filter("Images", &["png", "jpg", "jpeg", "tiff", "bmp"])
                .pick_file()
            {
                self.perform_slipsheet_desktop(path, req);
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::closure::Closure;
            use wasm_bindgen::JsCast;

            let document = web_sys::window().unwrap().document().unwrap();
            let input = document
                .create_element("input")
                .unwrap()
                .dyn_into::<web_sys::HtmlInputElement>()
                .unwrap();
            input.set_attribute("type", "file").unwrap();
            input.set_attribute("style", "display:none").unwrap();

            let tx = self.tx.clone();
            let ctx = _ctx.clone();
            let req_clone = req.clone();
            let project_id = self.current_project_id;

            let closure = Closure::wrap(Box::new(move |event: web_sys::Event| {
                let input: web_sys::HtmlInputElement = event.target().unwrap().dyn_into().unwrap();
                if let Some(files) = input.files() {
                    if let Some(file) = files.get(0) {
                        let name = file.name();
                        let tx = tx.clone();
                        let ctx = ctx.clone();
                        let req = req_clone.clone();

                        let reader = web_sys::FileReader::new().unwrap();
                        let reader_clone = reader.clone();

                        let onload = Closure::wrap(Box::new(move |_e: web_sys::Event| {
                            let result = reader_clone.result().unwrap();
                            let array_buffer = result.dyn_into::<js_sys::ArrayBuffer>().unwrap();
                            let uint8_array = js_sys::Uint8Array::new(&array_buffer);
                            let mut data = vec![0; uint8_array.length() as usize];
                            uint8_array.copy_to(&mut data);

                            let _ = tx.send(AppMessage::SlipSheetFileLoaded {
                                project_id,
                                name: name.clone(),
                                data,
                                req: req.clone(),
                            });
                            ctx.request_repaint();
                        }) as Box<dyn FnMut(_)>);

                        reader.set_onload(Some(onload.as_ref().unchecked_ref()));
                        reader.read_as_array_buffer(&file).unwrap();
                        onload.forget();
                    }
                }
            }) as Box<dyn FnMut(_)>);

            input
                .add_event_listener_with_callback("change", closure.as_ref().unchecked_ref())
                .unwrap();
            closure.forget();

            document.body().unwrap().append_child(&input).unwrap();
            input.click();
        }
    }

    /// Decode documents off the desktop UI thread. Results stay attached to their project.
    fn load_file(&mut self, path: PathBuf) {
        self.state.ui.pending_operations += 1;
        self.state.ui.is_loading = true;
        self.state
            .ui
            .set_status(format!("Loading {}…", path.display()));
        let registry = self.state.loader_registry.clone();
        let tx = self.tx.clone();
        let ctx = self.egui_ctx.clone();
        let project_id = self.current_project_id;
        let work = move || {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Document")
                .to_string();
            let result = if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
            {
                dc_core::load_pdf_all_pages(&path, dc_core::DEFAULT_PDF_DPI).map(|pages| {
                    let multiple = pages.len() > 1;
                    pages
                        .into_iter()
                        .map(|(page, buffer)| {
                            (
                                if multiple {
                                    format!("{name} - Page {page}")
                                } else {
                                    name.clone()
                                },
                                buffer,
                            )
                        })
                        .collect()
                })
            } else {
                registry
                    .load(&path, &LoadConfig::default())
                    .map(|buffer| vec![(name, buffer)])
            };
            let _ = tx.send(AppMessage::DocumentsLoaded {
                project_id,
                path,
                result,
            });
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        };
        #[cfg(not(target_arch = "wasm32"))]
        spawn_loading(work);
        #[cfg(target_arch = "wasm32")]
        work();
    }

    fn perform_slipsheet_desktop(
        &mut self,
        new_path: PathBuf,
        req: crate::state::SlipSheetRequest,
    ) {
        self.state
            .ui
            .set_status(crate::i18n::tr("status.slipsheet"));
        info!("Slip-sheeting started: req={:?}", req);

        let dpi = dc_core::DEFAULT_PDF_DPI;
        let is_pdf = new_path
            .extension()
            .map(|e| e.to_ascii_lowercase() == "pdf")
            .unwrap_or(false);

        // Load new buffers
        let mut new_pages = Vec::new();
        if is_pdf {
            if let Ok(pages) = dc_core::load_pdf_all_pages(&new_path, dpi) {
                new_pages = pages.into_iter().map(|(_, b)| b).collect();
            }
        } else {
            if let Ok(b) = self
                .state
                .loader_registry
                .load(&new_path, &LoadConfig::default())
            {
                new_pages.push(b);
            }
        }

        if new_pages.is_empty() {
            self.state
                .ui
                .set_status(crate::i18n::tr("status.slipsheet_fail"));
            return;
        }

        self.execute_slipsheet_replacement(new_pages, new_path, req);
    }

    fn perform_slipsheet_memory(
        &mut self,
        data: Vec<u8>,
        name: &str,
        req: crate::state::SlipSheetRequest,
    ) {
        self.state
            .ui
            .set_status(crate::i18n::tr("status.slipsheet"));

        let config = LoadConfig::default();
        let mut new_pages = Vec::new();
        if let Ok(b) = self
            .state
            .loader_registry
            .load_from_memory(&data, name, &config)
        {
            new_pages.push(b);
        }

        if new_pages.is_empty() {
            self.state
                .ui
                .set_status("Failed to load new file for slip-sheeting");
            return;
        }

        self.execute_slipsheet_replacement(new_pages, PathBuf::from(name), req);
    }

    fn execute_slipsheet_replacement(
        &mut self,
        new_pages: Vec<dc_core::RasterBuffer>,
        new_path: PathBuf,
        req: crate::state::SlipSheetRequest,
    ) {
        use crate::state::SlipSheetRequest;

        // Determine which layer IDs to replace
        let to_replace = match req {
            SlipSheetRequest::Single(id) => vec![id],
            SlipSheetRequest::Batch(ref doc_path) => self
                .state
                .session
                .layers
                .iter()
                .filter(|l| &l.source_path == doc_path)
                .map(|l| l.id)
                .collect(),
        };

        let mut replaced_count = 0;

        for (i, target_layer_id) in to_replace.into_iter().enumerate() {
            let next_buffer = new_pages.get(i).or_else(|| new_pages.last()); // Fallback to last page if fewer new pages than old
            if let Some(new_buffer) = next_buffer {
                // We need to fetch the existing layer
                let old_layer = if let Some(l) = self.state.session.get_layer(target_layer_id) {
                    l.clone()
                } else {
                    continue;
                };

                // Create a temporary unaligned new layer so we can align it AGAINST the old layer
                // Old layer is Reference, New layer is Target
                let engine = &self.state.alignment_engine;
                let alignment_result = engine.align(&old_layer.original, new_buffer);

                let mut final_annotations = old_layer.annotations.clone();

                if let Ok(res) = alignment_result {
                    // Try to map annotations. res.homography maps Target -> Reference
                    // But we want to map annotations from Reference (Old) to Target (New).
                    // So we need reverse homography: Reference -> Target.
                    if let Ok(inv_h) = res.homography.inverse() {
                        for annot in &mut final_annotations {
                            annot.transform_by(&inv_h);
                        }
                        info!(
                            "Transformed {} annotations during slipsheet",
                            final_annotations.len()
                        );
                    } else {
                        tracing::warn!("Homography inversion failed during slipsheet, annotations remain at original coords");
                    }
                } else {
                    tracing::warn!(
                        "Alignment failed during slipsheet, annotations remain at original coords"
                    );
                }

                // Update the layer with the new image and transformed annotations
                if let Some(layer_mut) = self.state.session.get_layer_mut(target_layer_id) {
                    // We invalidate its alignment against the global reference layer, since the image changed
                    layer_mut.aligned = None;
                    layer_mut.homography_matrix = None;
                    layer_mut.alignment_confidence = None;

                    layer_mut.original = std::sync::Arc::new(new_buffer.clone());
                    layer_mut.source_path = new_path.clone();
                    layer_mut.annotations = final_annotations;
                    layer_mut.name = new_path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown")
                        .to_string();
                }

                self.textures.remove(target_layer_id.0 as u64);

                // Now we need to align the newly updated layer against the global reference (if it's not the reference itself)
                let is_ref = self
                    .state
                    .session
                    .get_layer(target_layer_id)
                    .map(|l| l.is_reference)
                    .unwrap_or(false);
                if !is_ref {
                    self.align_layer(target_layer_id);
                }

                replaced_count += 1;
            }
        }

        self.state.ui.diff_invalidated = true;
        self.state.session.is_dirty = true;
        self.state.ui.set_status(format!(
            "Slip-sheeting completed: replaced {} layers",
            replaced_count
        ));
    }

    /// Open a file dialog to select a session file.
    fn open_session_dialog(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("DiffComp Session", &["dcs"])
            .pick_file()
        {
            self.load_session_from_path(&path);
        }

        #[cfg(target_arch = "wasm32")]
        {
            self.state
                .ui
                .set_status(crate::i18n::tr("status.session_load_web"));
        }
    }

    fn load_session_from_path(&mut self, path: &Path) {
        match crate::persistence::load_session(path) {
            Ok(session) => {
                self.state.session = session;
                self.state.ui.diff_invalidated = true;
                self.textures.clear();
                self.state
                    .ui
                    .set_status(crate::i18n::tr("status.session_loaded"));
            }
            Err(e) => {
                error!("Failed to load session: {}", e);
                self.state
                    .ui
                    .set_status(format!("Failed to load session: {}", e));
            }
        }
    }

    /// Save the current session.
    fn save_session(&mut self) {
        if let Some(path) = &self.state.session.session_path {
            match crate::persistence::save_session(&self.state.session, path) {
                Ok(_) => {
                    self.state.session.is_dirty = false;
                    self.state
                        .ui
                        .set_status(crate::i18n::tr("status.session_saved"));
                }
                Err(e) => {
                    error!("Failed to save session: {}", e);
                    self.state
                        .ui
                        .set_status(&format!("Failed to save session: {}", e));
                }
            }
        } else {
            // Open file dialog to save
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("DiffComp Session", &["dcs"])
                .save_file()
            {
                match crate::persistence::save_session(&self.state.session, &path) {
                    Ok(_) => {
                        self.state.session.session_path = Some(path);
                        self.state.session.is_dirty = false;
                        self.state
                            .ui
                            .set_status(crate::i18n::tr("status.session_saved"));
                    }
                    Err(e) => {
                        error!("Failed to save session: {}", e);
                        self.state
                            .ui
                            .set_status(&format!("Failed to save session: {}", e));
                    }
                }
            }

            #[cfg(target_arch = "wasm32")]
            {
                self.state
                    .ui
                    .set_status(crate::i18n::tr("status.session_save_web"));
            }
        }
    }

    /// Save session to a new path (Save As).
    fn save_session_as(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("DiffComp Session", &["dcs"])
            .save_file()
        {
            match crate::persistence::save_session(&self.state.session, &path) {
                Ok(_) => {
                    self.state.session.session_path = Some(path);
                    self.state.session.is_dirty = false;
                    self.state
                        .ui
                        .set_status(crate::i18n::tr("status.session_saved"));
                }
                Err(e) => {
                    error!("Failed to save session: {}", e);
                    self.state
                        .ui
                        .set_status(&format!("Failed to save session: {}", e));
                }
            }
        }
    }

    /// Export flattened (composited) image of all visible layers to PNG.
    fn export_flattened_image(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(composite) = self.state.session.composite_visible_layers_pub() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("PNG Image", &["png"])
                    .set_file_name("flattened_export.png")
                    .save_file()
                {
                    match composite.image.save(&path) {
                        Ok(_) => self
                            .state
                            .ui
                            .set_status(format!("Exported to {}", path.display())),
                        Err(e) => self.state.ui.set_status(format!("Export failed: {}", e)),
                    }
                }
            } else {
                self.state
                    .ui
                    .set_status(crate::i18n::tr("status.no_layers_export"));
            }
        }
    }

    /// Export the current diff result to PNG.
    fn export_diff_image(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(diff) = &self.state.session.diff_result {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("PNG Image", &["png"])
                    .set_file_name("diff_export.png")
                    .save_file()
                {
                    match diff.image.save(&path) {
                        Ok(_) => self
                            .state
                            .ui
                            .set_status(format!("Diff exported to {}", path.display())),
                        Err(e) => self
                            .state
                            .ui
                            .set_status(format!("Diff export failed: {}", e)),
                    }
                }
            } else {
                self.state
                    .ui
                    .set_status(crate::i18n::tr("status.no_diff_export"));
            }
        }
    }

    /// Export the currently selected layer to PNG.
    fn export_selected_layer(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let image = self.state.session.selected_layer.and_then(|id| {
                self.state
                    .session
                    .get_layer(id)
                    .map(|l| l.active_image().clone())
            });
            if let Some(img) = image {
                let layer_name = self
                    .state
                    .session
                    .selected_layer
                    .and_then(|id| self.state.session.get_layer(id))
                    .map(|l| l.name.clone())
                    .unwrap_or_else(|| "layer".to_string());
                let safe_name =
                    layer_name.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("PNG Image", &["png"])
                    .set_file_name(format!("{}_export.png", safe_name))
                    .save_file()
                {
                    match img.image.save(&path) {
                        Ok(_) => self
                            .state
                            .ui
                            .set_status(format!("Layer exported to {}", path.display())),
                        Err(e) => self
                            .state
                            .ui
                            .set_status(format!("Layer export failed: {}", e)),
                    }
                }
            } else {
                self.state
                    .ui
                    .set_status(crate::i18n::tr("status.no_layer_export"));
            }
        }
    }

    /// Perform alignment on all target layers.
    fn perform_alignment(&mut self) {
        if !self.state.session.can_compare() {
            self.state
                .ui
                .set_status(crate::i18n::tr("status.need_2_layers"));
            return;
        }

        self.state.ui.set_status(crate::i18n::tr("status.aligning"));
        self.state.ui.is_loading = true;

        let reference = match self.state.session.reference_layer() {
            Some(r) => r.original.clone(),
            None => {
                self.state
                    .ui
                    .set_status(crate::i18n::tr("status.no_ref_layer"));
                self.state.ui.is_loading = false;
                return;
            }
        };

        // Align each target layer
        let target_ids: Vec<_> = self
            .state
            .session
            .target_layers()
            .iter()
            .map(|l| l.id)
            .collect();

        for id in target_ids {
            self.align_single_layer(id, &reference);
        }

        self.state.ui.is_loading = self.state.ui.pending_operations > 0;
    }

    /// Align a single layer against the reference (called when a new layer is added).
    fn align_layer(&mut self, id: dc_core::LayerId) {
        // Skip if this is the reference layer
        if let Some(layer) = self.state.session.get_layer(id) {
            if layer.is_reference {
                return;
            }
        }

        let reference = match self.state.session.reference_layer() {
            Some(r) => r.original.clone(),
            None => return,
        };

        self.align_single_layer(id, &reference);
    }

    /// Internal: align a single layer against a reference image.
    ///
    /// Uses force-fit alignment (content-box affine transform) ported from the
    /// Python compareTIFF.py implementation. This works much better for
    /// engineering/technical drawings than feature-based alignment.
    ///
    /// If the target image is larger than the reference, the output canvas
    /// is expanded to the union of both dimensions. In that case, the reference
    /// layer also gets an `aligned` image (padded with white) so that both
    /// layers share the same canvas size for accurate diff computation.
    fn align_single_layer(&mut self, id: dc_core::LayerId, _reference: &dc_core::RasterBuffer) {
        let Some(reference_layer) = self.state.session.reference_layer() else {
            return;
        };
        let Some(layer) = self.state.session.get_layer(id) else {
            return;
        };
        let reference_id = reference_layer.id;
        let reference = reference_layer.original.clone();
        let target = layer.original.clone();
        let tx = self.tx.clone();
        let ctx = self.egui_ctx.clone();
        let project_id = self.current_project_id;
        self.state.ui.pending_operations += 1;
        self.state.ui.is_loading = true;
        let work = move || {
            let result = dc_core::force_fit_align(&reference, &target, true);
            let _ = tx.send(AppMessage::AlignmentComputed {
                project_id,
                layer_id: id,
                reference_id,
                reference,
                target,
                result,
            });
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        };
        #[cfg(not(target_arch = "wasm32"))]
        spawn_processing(work);
        #[cfg(target_arch = "wasm32")]
        work();
    }

    fn apply_alignment_result(
        &mut self,
        id: dc_core::LayerId,
        reference_id: dc_core::LayerId,
        result: dc_core::CoreResult<dc_core::ForceFitResult>,
    ) {
        match result {
            Ok(result) => {
                // Completing another target must never shrink the shared reference canvas.
                if let Some(reference) = self.state.session.get_layer_mut(reference_id) {
                    let current = reference.active_image().dimensions();
                    let next = result.reference.dimensions();
                    if next.0 > current.0 || next.1 > current.1 {
                        let mut canvas = image::RgbaImage::from_pixel(
                            current.0.max(next.0),
                            current.1.max(next.1),
                            image::Rgba([255; 4]),
                        );
                        image::imageops::replace(&mut canvas, &reference.original.image, 0, 0);
                        reference.aligned = Some(std::sync::Arc::new(dc_core::RasterBuffer::new(
                            canvas,
                            reference.original.dpi,
                        )));
                        self.textures.remove(reference_id.0 as u64);
                    }
                }
                if let Some(layer) = self.state.session.get_layer_mut(id) {
                    layer.homography_matrix = Some(result.homography);
                    layer.aligned = Some(std::sync::Arc::new(result.aligned_target));
                    layer.alignment_confidence = Some(1.0);
                }
                self.textures.remove(id.0 as u64);
                self.state.ui.diff_invalidated = true;
                self.state
                    .ui
                    .set_status(crate::i18n::tr("status.alignment_complete"));
            }
            Err(e) => self.state.ui.set_status(format!("Alignment failed: {e}")),
        }
    }

    /// Compute the difference visualization.
    ///
    /// Uses morphological tolerance diff (ported from Python compareTIFF.py).
    /// This approach binarizes both images, applies 3x3 dilation for tolerance,
    /// and finds unique ink per image. Falls back to GPU if morphological fails.
    fn compute_diff(&mut self) {
        if self.state.ui.is_loading || self.state.ui.diff_running.is_some() {
            return;
        }

        // Get reference and visible targets
        let reference_dpi = self
            .state
            .session
            .visible_reference()
            .map(|r| r.active_image().dpi)
            .unwrap_or(300);
        let reference_offset = self
            .state
            .session
            .visible_reference()
            .map(|r| [r.offset_x, r.offset_y])
            .unwrap_or([0.0; 2]);
        let reference_image = match self.state.session.visible_reference() {
            Some(r) => r.active_buffer(),
            None => {
                self.state
                    .ui
                    .set_status(crate::i18n::tr("status.no_visible_ref"));
                return;
            }
        };

        let visible_targets = self.state.session.visible_target_layers();
        if visible_targets.is_empty() {
            self.state
                .ui
                .set_status(crate::i18n::tr("status.no_visible_targets"));
            return;
        }

        let target_images: Vec<std::sync::Arc<dc_core::RasterBuffer>> =
            visible_targets.iter().map(|l| l.active_buffer()).collect();

        // Get reference color (default: red for ref)
        let ref_color: [u8; 3] = [
            self.state.session.diff_config.reference_color.r,
            self.state.session.diff_config.reference_color.g,
            self.state.session.diff_config.reference_color.b,
        ];

        // Collect per-layer blend colors and offsets for GPU fallback
        let target_colors: Vec<[f32; 3]> = visible_targets
            .iter()
            .map(|l| {
                [
                    l.blend_color.r as f32 / 255.0,
                    l.blend_color.g as f32 / 255.0,
                    l.blend_color.b as f32 / 255.0,
                ]
            })
            .collect();

        let target_offsets: Vec<[f32; 2]> = visible_targets
            .iter()
            .map(|l| {
                [
                    l.offset_x - reference_offset[0],
                    l.offset_y - reference_offset[1],
                ]
            })
            .collect();

        // Sync DiffConfig → GPU params (for fallback)
        self.state.gpu_params.blend_mode = match self.state.session.diff_config.blend_mode {
            dc_core::diff::BlendMode::Overlay => dc_gpu::DiffBlendMode::Overlay,
            dc_core::diff::BlendMode::ColorDifference => dc_gpu::DiffBlendMode::ColorDifference,
            dc_core::diff::BlendMode::Heatmap => dc_gpu::DiffBlendMode::Heatmap,
            dc_core::diff::BlendMode::BinaryMask => dc_gpu::DiffBlendMode::Binary,
            dc_core::diff::BlendMode::Subtract => dc_gpu::DiffBlendMode::Subtract,
            dc_core::diff::BlendMode::Xor => dc_gpu::DiffBlendMode::Xor,
        };

        self.state.gpu_params.threshold =
            if self.state.session.diff_config.blend_mode == dc_core::diff::BlendMode::BinaryMask {
                self.state.session.diff_config.binary_threshold as f32 / 255.0
            } else {
                self.state.session.diff_config.noise_threshold as f32 / 255.0
            };

        self.state.gpu_params.opacity = self.state.session.diff_config.overlay_opacity;
        self.state.gpu_params.context_opacity = 1.0;

        self.state.gpu_params.ref_color = [
            self.state.session.diff_config.reference_color.r as f32 / 255.0,
            self.state.session.diff_config.reference_color.g as f32 / 255.0,
            self.state.session.diff_config.reference_color.b as f32 / 255.0,
        ];

        let gpu_params = self.state.gpu_params.clone();
        let cpu_config = self.state.session.diff_config.clone();
        let gpu_engine_opt = self.state.gpu_engine.clone();

        let tx = self.tx.clone();
        let repaint_ctx = self.egui_ctx.clone();
        let project_id = self.current_project_id;

        self.diff_generation += 1;
        let generation = self.diff_generation;
        self.state.ui.diff_running = Some(generation);
        self.state.ui.diff_failed = false;

        self.state.ui.is_loading = true;
        self.state
            .ui
            .set_status(crate::i18n::tr("status.computing_diff"));

        let compute_mode = self.state.compute_mode;

        let work = async move {
            let target_refs: Vec<&image::RgbaImage> =
                target_images.iter().map(|b| &b.image).collect();
            let colors: Vec<[u8; 3]> = target_colors
                .iter()
                .map(|c| c.map(|v| (v * 255.0).round() as u8))
                .collect();
            let morphology = cpu_config.morphological_tolerance
                && cpu_config.blend_mode == dc_core::diff::BlendMode::ColorDifference
                && compute_mode != ComputeMode::Gpu;
            let result = if morphology {
                dc_core::diff::compute_morphological_diff_batch(
                    &reference_image.image,
                    &target_refs,
                    &colors,
                    &target_offsets,
                    ref_color,
                )
                .map(|r| r.image.image)
                .map_err(|e| e.to_string())
            } else {
                let gpu_result = if compute_mode != ComputeMode::Cpu {
                    if let Some(gpu) = gpu_engine_opt {
                        Some(
                            gpu.compute_diff_batch(
                                &reference_image.image,
                                &target_refs,
                                &target_colors,
                                &target_offsets,
                                &gpu_params,
                            )
                            .await,
                        )
                    } else {
                        None
                    }
                } else {
                    None
                };
                match gpu_result {
                    Some(Ok(image)) => Ok(image),
                    other => {
                        if let Some(Err(e)) = other {
                            error!("GPU diff failed, using CPU: {e}");
                        }
                        dc_core::diff::compute_cpu_diff_batch(
                            &reference_image.image,
                            &target_refs,
                            &colors,
                            &target_offsets,
                            &cpu_config,
                        )
                        .map_err(|e| e.to_string())
                    }
                }
            };
            let message = match result {
                Ok(image) => AppMessage::DiffComputed {
                    project_id,
                    generation,
                    result: dc_core::RasterBuffer::new(image, reference_dpi),
                },
                Err(error) => AppMessage::DiffFailed {
                    project_id,
                    generation,
                    error,
                },
            };
            let _ = tx.send(message);
            if let Some(ctx) = repaint_ctx {
                ctx.request_repaint();
            }
        };

        // Spawn task
        #[cfg(target_arch = "wasm32")]
        {
            wasm_bindgen_futures::spawn_local(work);
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            spawn_processing(move || {
                pollster::block_on(work);
            });
        }
    }

    /// Primary comparison actions stay visible, independent of the inspector tabs.
    fn workspace_toolbar(&mut self, ctx: &Context) {
        use dc_core::diff::BlendMode;
        egui::TopBottomPanel::top("workspace_controls")
            .frame(
                egui::Frame::none()
                    .fill(crate::theme::BAR)
                    .inner_margin(egui::Margin::symmetric(12.0, 7.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .button(crate::i18n::tr("workspace.open"))
                        .on_hover_text("Open documents · Cmd/Ctrl+O")
                        .clicked()
                    {
                        self.state.ui.request_file_dialog = true;
                    }
                    ui.separator();
                    let can_compare = self.state.session.can_compare();
                    if ui
                        .add_enabled(
                            can_compare && self.state.ui.pending_operations == 0,
                            egui::Button::new(crate::i18n::tr("workspace.align")),
                        )
                        .on_hover_text("Automatically align drawings")
                        .clicked()
                    {
                        self.perform_alignment();
                    }
                    let before = self.state.session.diff_config.blend_mode;
                    egui::ComboBox::from_id_salt("quick_comparison_mode")
                        .width(138.0)
                        .selected_text(match before {
                            BlendMode::ColorDifference => "Color difference",
                            BlendMode::Overlay => "Overlay",
                            BlendMode::Heatmap => "Heatmap",
                            BlendMode::BinaryMask => "Binary mask",
                            BlendMode::Subtract => "Subtract",
                            BlendMode::Xor => "XOR",
                        })
                        .show_ui(ui, |ui| {
                            for (mode, name) in [
                                (BlendMode::ColorDifference, "Color difference"),
                                (BlendMode::Overlay, "Overlay"),
                                (BlendMode::Heatmap, "Heatmap"),
                                (BlendMode::BinaryMask, "Binary mask"),
                                (BlendMode::Subtract, "Subtract"),
                                (BlendMode::Xor, "XOR"),
                            ] {
                                ui.selectable_value(
                                    &mut self.state.session.diff_config.blend_mode,
                                    mode,
                                    name,
                                );
                            }
                        });
                    if before != self.state.session.diff_config.blend_mode {
                        self.state.ui.diff_invalidated = true;
                    }
                    let label = if self.state.ui.diff_running.is_some() {
                        crate::i18n::tr("workspace.comparing")
                    } else {
                        crate::i18n::tr("workspace.compare")
                    };
                    if ui
                        .add_enabled(
                            can_compare && !self.state.ui.is_loading,
                            egui::Button::new(
                                egui::RichText::new(label).color(egui::Color32::WHITE),
                            )
                            .fill(egui::Color32::from_rgb(48, 109, 192)),
                        )
                        .clicked()
                    {
                        self.compute_diff();
                    }
                    ui.checkbox(&mut self.state.ui.auto_diff_enabled, "Auto")
                        .on_hover_text("Update differences after document or setting changes");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .button(crate::i18n::tr("workspace.fit"))
                            .on_hover_text("Fit document to canvas (F)")
                            .clicked()
                        {
                            self.state.ui.fit_view_requested = true;
                        }
                        if ui.small_button("+").clicked() {
                            self.state.session.viewport.apply_zoom(1.2);
                        }
                        ui.label(format!("{:.0}%", self.state.session.viewport.zoom * 100.0));
                        if ui.small_button("−").clicked() {
                            self.state.session.viewport.apply_zoom(1.0 / 1.2);
                        }
                        ui.separator();
                        if ui
                            .selectable_label(
                                self.dock_state.find_tab(&Tab::Markups).is_some(),
                                crate::i18n::tr("workspace.markups"),
                            )
                            .clicked()
                        {
                            if let Some((surface, node, index)) =
                                self.dock_state.find_tab(&Tab::Markups)
                            {
                                self.dock_state.remove_tab((surface, node, index));
                            } else if let Some((surface, node, _)) =
                                self.dock_state.find_tab(&Tab::Viewport(0))
                            {
                                self.dock_state[surface].split_below(
                                    node,
                                    0.76,
                                    vec![Tab::Markups],
                                );
                            }
                        }
                    });
                });
            });
    }

    /// Check if auto-diff should be triggered and compute if needed.
    /// Auto-diff triggers when:
    /// 1. Auto-diff is enabled
    /// 2. We can compare (2+ visible layers with a visible reference)
    /// 3. No diff result is currently displayed
    fn check_auto_diff(&mut self) {
        if self.state.ui.auto_diff_enabled
            && self.state.session.can_compare()
            && self.state.session.diff_result.is_none()
            && !self.state.ui.diff_failed
            && self.state.ui.diff_running.is_none()
            && self
                .state
                .ui
                .diff_changed_at
                .is_none_or(|at| at.elapsed() >= std::time::Duration::from_millis(150))
        {
            self.compute_diff();
        }
    }
}

impl eframe::App for DiffCompApp {
    fn update(&mut self, ctx: &Context, _frame: &mut Frame) {
        // Cache the egui context so background threads can request repaints
        if self.egui_ctx.is_none() {
            self.egui_ctx = Some(ctx.clone());
        }
        // 1. Handle File Dialog Requests from UI (e.g. Layers Panel)
        if self.state.ui.request_file_dialog {
            self.state.ui.request_file_dialog = false;
            self.open_file_dialog(ctx);
        }

        if let Some(req) = self.state.ui.request_slipsheet_dialog.take() {
            self.open_slipsheet_dialog(ctx, req);
        }

        // Handle export requests from menus
        if self.state.ui.request_save_as {
            self.state.ui.request_save_as = false;
            self.save_session_as();
        }
        if self.state.ui.export_flattened_requested {
            self.state.ui.export_flattened_requested = false;
            self.export_flattened_image();
        }
        if self.state.ui.export_diff_requested {
            self.state.ui.export_diff_requested = false;
            self.export_diff_image();
        }
        if self.state.ui.export_layer_requested {
            self.state.ui.export_layer_requested = false;
            self.export_selected_layer();
        }
        if self.state.ui.export_markups_csv_requested {
            self.state.ui.export_markups_csv_requested = false;
            self.state.ui.markups_list.export_csv_requested = true;
        }
        if self.state.ui.export_markups_xlsx_requested {
            self.state.ui.export_markups_xlsx_requested = false;
            self.state.ui.markups_list.export_xlsx_requested = true;
        }

        // 2. Handle Drag & Drop
        if !ctx.input(|i| i.raw.dropped_files.is_empty()) {
            let dropped_files = ctx.input(|i| i.raw.dropped_files.clone());
            for file in dropped_files {
                if let Some(path) = file.path {
                    #[cfg(not(target_arch = "wasm32"))]
                    if path
                        .extension()
                        .and_then(|s| s.to_str())
                        .map(|s| s.eq_ignore_ascii_case("dcs"))
                        .unwrap_or(false)
                    {
                        match crate::persistence::load_session(&path) {
                            Ok(session) => {
                                let name = path
                                    .file_stem()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("Imported Project")
                                    .to_string();
                                self.create_project_tab_from_session(name, session);
                            }
                            Err(e) => {
                                self.state
                                    .ui
                                    .set_status(format!("Failed to import dropped session: {}", e));
                            }
                        }
                        continue;
                    }

                    // Desktop case: File has a path
                    self.load_file(path);
                } else if let Some(bytes) = file.bytes {
                    // Web case: File has bytes (and name)
                    let name = file.name;
                    let data = bytes.to_vec();
                    // Inject into async message queue to reuse loading logic
                    let _ = self.tx.send(AppMessage::FileLoaded {
                        project_id: self.current_project_id,
                        name,
                        data,
                    });
                }
            }
        }

        // Check for async messages from web
        self.check_messages();

        // Receive project transfers from other app instances.
        #[cfg(not(target_arch = "wasm32"))]
        if self.last_ipc_poll.elapsed() >= std::time::Duration::from_millis(250) {
            self.process_incoming_ipc_messages();
            self.last_ipc_poll = std::time::Instant::now();
        }
        #[cfg(not(target_arch = "wasm32"))]
        ctx.request_repaint_after(std::time::Duration::from_millis(250));

        #[cfg(not(target_arch = "wasm32"))]
        self.maybe_flush_pending_spawn();

        // While a cross-instance drag is in flight, poll the inbox frequently
        // so a "remove_project" ACK from the target cancels the fallback spawn
        // before it fires.
        #[cfg(not(target_arch = "wasm32"))]
        if self.pending_spawn_project_id.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }

        // Free textures from previous frame
        self.textures.process_garbage();

        // Handle keyboard shortcuts
        self.handle_shortcuts(ctx);

        // =====================================================================
        // Top Menu Bar
        // =====================================================================
        egui::TopBottomPanel::top("menu_bar")
            .frame(
                egui::Frame::none()
                    .fill(crate::theme::BAR)
                    .inner_margin(egui::Margin::symmetric(10.0, 4.0)),
            )
            .show(ctx, |ui| {
                egui::menu::bar(ui, |ui| {
                    // File menu
                    ui.menu_button(crate::i18n::tr("menu.file"), |ui| {
                        if ui.button(crate::i18n::tr("file.open")).clicked() {
                            self.open_file_dialog(ui.ctx());
                            ui.close_menu();
                        }
                        if ui.button(crate::i18n::tr("file.import_image")).clicked() {
                            self.open_file_dialog(ui.ctx());
                            ui.close_menu();
                        }
                        if ui.button(crate::i18n::tr("file.open_session")).clicked() {
                            self.open_session_dialog();
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button(crate::i18n::tr("file.save_session")).clicked() {
                            self.save_session();
                            ui.close_menu();
                        }
                        if ui.button(crate::i18n::tr("file.save_session_as")).clicked() {
                            self.state.ui.request_save_as = true;
                            ui.close_menu();
                        }
                        ui.separator();
                        ui.menu_button(crate::i18n::tr("file.export"), |ui| {
                            if ui
                                .add_enabled(
                                    !self.state.session.layers.is_empty(),
                                    egui::Button::new(crate::i18n::tr("file.export_flattened")),
                                )
                                .clicked()
                            {
                                self.state.ui.export_flattened_requested = true;
                                ui.close_menu();
                            }
                            if ui
                                .add_enabled(
                                    self.state.session.diff_result.is_some(),
                                    egui::Button::new(crate::i18n::tr("file.export_diff")),
                                )
                                .clicked()
                            {
                                self.state.ui.export_diff_requested = true;
                                ui.close_menu();
                            }
                            if ui
                                .add_enabled(
                                    self.state.session.selected_layer.is_some(),
                                    egui::Button::new(crate::i18n::tr("file.export_layer")),
                                )
                                .clicked()
                            {
                                self.state.ui.export_layer_requested = true;
                                ui.close_menu();
                            }
                            ui.separator();
                            if ui.button(crate::i18n::tr("file.export_csv")).clicked() {
                                self.state.ui.export_markups_csv_requested = true;
                                ui.close_menu();
                            }
                            if ui.button(crate::i18n::tr("file.export_xlsx")).clicked() {
                                self.state.ui.export_markups_xlsx_requested = true;
                                ui.close_menu();
                            }
                        });
                        ui.separator();
                        if ui.button(crate::i18n::tr("file.settings")).clicked() {
                            self.state.ui.show_settings_dialog = true;
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button(crate::i18n::tr("file.exit")).clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });

                    // Edit menu
                    ui.menu_button(crate::i18n::tr("menu.edit"), |ui| {
                        let undo_label =
                            if let Some(desc) = self.state.session.undo_stack.undo_description() {
                                format!("Undo: {}  (Ctrl+Z)", desc)
                            } else {
                                crate::i18n::tr("edit.undo").to_string()
                            };
                        if ui
                            .add_enabled(
                                self.state.session.undo_stack.can_undo(),
                                egui::Button::new(undo_label),
                            )
                            .clicked()
                        {
                            self.perform_undo();
                            ui.close_menu();
                        }

                        let redo_label =
                            if let Some(desc) = self.state.session.undo_stack.redo_description() {
                                format!("Redo: {}  (Ctrl+Shift+Z)", desc)
                            } else {
                                crate::i18n::tr("edit.redo").to_string()
                            };
                        if ui
                            .add_enabled(
                                self.state.session.undo_stack.can_redo(),
                                egui::Button::new(redo_label),
                            )
                            .clicked()
                        {
                            self.perform_redo();
                            ui.close_menu();
                        }
                    });

                    // View menu
                    ui.menu_button(crate::i18n::tr("menu.view"), |ui| {
                        // Toggle Layers Panel
                        let layers_tab = Tab::Layers;
                        let mut layers_visible = self.dock_state.find_tab(&layers_tab).is_some();
                        if ui
                            .checkbox(&mut layers_visible, crate::i18n::tr("view.layer_panel"))
                            .clicked()
                        {
                            if layers_visible {
                                // Was hidden, now show (add to left of root)
                                self.dock_state.main_surface_mut().split_left(
                                    egui_dock::NodeIndex::root(),
                                    0.20,
                                    vec![layers_tab],
                                );
                            } else {
                                // Was visible, now hide (remove)
                                if let Some((surf, node, tab_idx)) =
                                    self.dock_state.find_tab(&layers_tab)
                                {
                                    let _ = self.dock_state.remove_tab((surf, node, tab_idx));
                                }
                            }
                        }

                        // Toggle Properties Panel
                        let props_tab = Tab::Properties;
                        let mut props_visible = self.dock_state.find_tab(&props_tab).is_some();
                        if ui
                            .checkbox(&mut props_visible, crate::i18n::tr("view.properties_panel"))
                            .clicked()
                        {
                            if props_visible {
                                // Show (add to right of root)
                                self.dock_state.main_surface_mut().split_right(
                                    egui_dock::NodeIndex::root(),
                                    0.25,
                                    vec![props_tab],
                                );
                            } else {
                                // Hide
                                if let Some((surf, node, tab_idx)) =
                                    self.dock_state.find_tab(&props_tab)
                                {
                                    let _ = self.dock_state.remove_tab((surf, node, tab_idx));
                                }
                            }
                        }

                        // Toggle Markups List Panel
                        let markups_tab = Tab::Markups;
                        let mut markups_visible = self.dock_state.find_tab(&markups_tab).is_some();
                        if ui
                            .checkbox(&mut markups_visible, crate::i18n::tr("view.markups_list"))
                            .clicked()
                        {
                            if markups_visible {
                                // Show (add to bottom of root)
                                self.dock_state.main_surface_mut().split_below(
                                    egui_dock::NodeIndex::root(),
                                    0.20,
                                    vec![markups_tab],
                                );
                            } else {
                                // Hide
                                if let Some((surf, node, tab_idx)) =
                                    self.dock_state.find_tab(&markups_tab)
                                {
                                    let _ = self.dock_state.remove_tab((surf, node, tab_idx));
                                }
                            }
                        }

                        // Toggle Minimap Panel
                        let minimap_tab = Tab::Minimap;
                        let mut minimap_visible = self.dock_state.find_tab(&minimap_tab).is_some();
                        if ui
                            .checkbox(&mut minimap_visible, crate::i18n::tr("view.minimap"))
                            .clicked()
                        {
                            if minimap_visible {
                                // Show (add to left panel area)
                                self.dock_state.main_surface_mut().split_left(
                                    egui_dock::NodeIndex::root(),
                                    0.15,
                                    vec![minimap_tab],
                                );
                            } else {
                                // Hide
                                if let Some((surf, node, tab_idx)) =
                                    self.dock_state.find_tab(&minimap_tab)
                                {
                                    let _ = self.dock_state.remove_tab((surf, node, tab_idx));
                                }
                            }
                        }

                        // Toggle Legend Panel
                        let legend_tab = Tab::Legend;
                        let mut legend_visible = self.dock_state.find_tab(&legend_tab).is_some();
                        if ui
                            .checkbox(&mut legend_visible, crate::i18n::tr("view.legend"))
                            .clicked()
                        {
                            if legend_visible {
                                self.dock_state.main_surface_mut().split_right(
                                    egui_dock::NodeIndex::root(),
                                    0.8,
                                    vec![legend_tab],
                                );
                            } else {
                                if let Some((surf, node, tab_idx)) =
                                    self.dock_state.find_tab(&legend_tab)
                                {
                                    let _ = self.dock_state.remove_tab((surf, node, tab_idx));
                                }
                            }
                        }

                        ui.separator();

                        if ui.button(crate::i18n::tr("view.split_viewport")).clicked() {
                            // Add a new unique viewport
                            // Find the max viewport ID to pick next
                            let next_id = self
                                .dock_state
                                .iter_all_tabs()
                                .filter_map(|(_, tab)| match tab {
                                    Tab::Viewport(id) => Some(*id),
                                    _ => None,
                                })
                                .max()
                                .unwrap_or(0)
                                + 1;

                            // Split the currently focused leaf, or root if none
                            self.dock_state.main_surface_mut().split_right(
                                egui_dock::NodeIndex::root(),
                                0.5,
                                vec![Tab::Viewport(next_id)],
                            );
                            ui.close_menu();
                        }

                        ui.separator();

                        if ui.button(crate::i18n::tr("view.zoom_in")).clicked() {
                            self.state.session.viewport.apply_zoom(1.25);
                            ui.close_menu();
                        }
                        if ui.button(crate::i18n::tr("view.zoom_out")).clicked() {
                            self.state.session.viewport.apply_zoom(0.8);
                            ui.close_menu();
                        }
                        if ui.button(crate::i18n::tr("view.zoom_100")).clicked() {
                            self.state.session.viewport.zoom = 1.0;
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button(crate::i18n::tr("view.fit_to_window")).clicked() {
                            if self.state.session.reference_layer().is_some() {
                                self.state.ui.fit_view_requested = true;
                            }
                            ui.close_menu();
                        }
                        if ui.button(crate::i18n::tr("view.reset_view")).clicked() {
                            self.state.session.viewport.reset();
                            ui.close_menu();
                        }
                    });

                    // Compare menu
                    ui.menu_button(crate::i18n::tr("menu.compare"), |ui| {
                        let can_compare = self.state.session.can_compare();

                        ui.checkbox(
                            &mut self.state.ui.auto_diff_enabled,
                            crate::i18n::tr("compare.auto_diff"),
                        );
                        ui.separator();
                        if ui
                            .add_enabled(
                                can_compare,
                                egui::Button::new(crate::i18n::tr("compare.align")),
                            )
                            .clicked()
                        {
                            self.perform_alignment();
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(
                                can_compare,
                                egui::Button::new(crate::i18n::tr("compare.compute_diff")),
                            )
                            .clicked()
                        {
                            self.compute_diff();
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(
                                self.state.session.diff_result.is_some(),
                                egui::Button::new(crate::i18n::tr("compare.clear_diff")),
                            )
                            .clicked()
                        {
                            self.diff_generation += 1;
                            self.state.ui.auto_diff_enabled = false;
                            self.state.session.diff_result = None;
                            const DIFF_TEXTURE_ID: u64 = u64::MAX;
                            self.textures.remove(DIFF_TEXTURE_ID);
                            ui.close_menu();
                        }
                    });

                    // Tools menu (calibration, counters)
                    ui.menu_button(crate::i18n::tr("menu.tools"), |ui| {
                        ui.label(crate::i18n::tr("tools.calibration"));
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label(crate::i18n::tr("tools.pixels_per_unit"));
                            ui.add(
                                egui::DragValue::new(
                                    &mut self.state.session.calibration.pixels_per_unit,
                                )
                                .speed(0.1)
                                .range(0.001..=1_000_000.0),
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.label(crate::i18n::tr("tools.unit"));
                            ui.text_edit_singleline(&mut self.state.session.calibration.unit);
                        });
                        ui.separator();
                        if ui.button(crate::i18n::tr("tools.reset_pixels")).clicked() {
                            self.state.session.calibration = dc_core::Calibration::default();
                            ui.close_menu();
                        }
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label(crate::i18n::tr("tools.count_counter"));
                            ui.add(
                                egui::DragValue::new(&mut self.state.session.count_counter)
                                    .range(1..=99999_u32),
                            );
                        });
                        if ui.button(crate::i18n::tr("tools.reset_counter")).clicked() {
                            self.state.session.count_counter = 1;
                            ui.close_menu();
                        }
                    });
                    ui.menu_button(crate::i18n::tr("menu.help"), |ui| {
                        if ui.button(crate::i18n::tr("help.about")).clicked() {
                            self.state.ui.show_about_dialog = true;
                            ui.close_menu();
                        }
                        if ui.button(crate::i18n::tr("help.license")).clicked() {
                            self.state.ui.show_license_dialog = true;
                            ui.close_menu();
                        }
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(crate::i18n::tr("dlg.settings_title")).clicked() {
                            self.state.ui.show_settings_dialog = true;
                        }
                    });
                });

                ui.separator();

                let project_tabs = self.project_tabs();
                let mut switch_to: Option<u32> = None;
                let mut close_tab: Option<u32> = None;
                let mut create_new = false;
                let mut swap_tabs: Option<(usize, usize)> = None;
                let mut project_row_rect: Option<egui::Rect> = None;

                ui.horizontal_wrapped(|ui| {
                    ui.add_space(5.0);

                    for (pos, (id, name, active)) in project_tabs.iter().enumerate() {
                        let button = egui::Button::new(egui::RichText::new(name).size(12.0))
                            .selected(*active)
                            .sense(egui::Sense::click_and_drag());
                        let response = ui.add(button);

                        if response.clicked() {
                            switch_to = Some(*id);
                        }

                        // Drag and drop support for reordering tabs
                        if response.drag_started_by(egui::PointerButton::Primary) {
                            ui.memory_mut(|m| {
                                m.data.insert_temp(egui::Id::new("dragging_tab"), pos)
                            });
                            #[cfg(not(target_arch = "wasm32"))]
                            self.create_drag_offer_for_project(*id);
                        }

                        // Handle drop target
                        if response.hovered() && ctx.dragged_id().is_some() {
                            if let Some(source_pos) = ui
                                .memory(|m| m.data.get_temp::<usize>(egui::Id::new("dragging_tab")))
                            {
                                if source_pos != pos
                                    && response.drag_stopped_by(egui::PointerButton::Primary)
                                {
                                    swap_tabs = Some((source_pos, pos));
                                    ui.memory_mut(|m| {
                                        m.data.remove::<usize>(egui::Id::new("dragging_tab"))
                                    });
                                }
                            }
                        }

                        if project_tabs.len() > 1 {
                            if ui
                                .small_button("×")
                                .on_hover_text("Close project")
                                .clicked()
                            {
                                close_tab = Some(*id);
                                // Clear any active drag operation
                                ui.memory_mut(|m| {
                                    m.data.remove::<usize>(egui::Id::new("dragging_tab"))
                                });
                            }
                        }
                    }

                    if ui.button("+").on_hover_text("New project").clicked() {
                        create_new = true;
                    }

                    project_row_rect = Some(ui.min_rect());
                });

                if let Some(id) = switch_to {
                    self.switch_to_project(id);
                }
                if let Some(id) = close_tab {
                    self.close_project_tab(id);
                }
                if let Some((pos1, pos2)) = swap_tabs {
                    self.swap_projects_by_position(pos1, pos2);
                }
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(rect) = project_row_rect {
                    self.maybe_accept_drag_offer(ctx, rect);
                    self.maybe_spawn_instance_from_drag_release(ctx, rect, swap_tabs.is_some());
                }
                if create_new {
                    self.create_project_tab();
                }
            });

        self.workspace_toolbar(ctx);
        panels::tool_rail(ctx, &mut self.state);

        // =====================================================================
        // Bottom Status Bar
        // =====================================================================
        egui::TopBottomPanel::bottom("status_bar")
            .frame(
                egui::Frame::none()
                    .fill(crate::theme::BAR)
                    .inner_margin(egui::Margin::symmetric(12.0, 5.0)),
            )
            .show(ctx, |ui| {
                ui.style_mut().override_text_style = Some(egui::TextStyle::Small);
                ui.horizontal(|ui| {
                    // Status message
                    ui.label(&self.state.ui.status_message);
                });
            });

        // =====================================================================
        // Left Panel: Layer Controls
        // =====================================================================
        // =====================================================================
        // Docking Area (Replaces Panels)
        // =====================================================================
        // Keep the inspector usable when the window is narrowed.
        let dock_width = (ctx.screen_rect().width() - 44.0).max(500.0);
        if let egui_dock::Node::Horizontal { fraction, .. } =
            &mut self.dock_state.main_surface_mut()[egui_dock::NodeIndex::root()]
        {
            *fraction = (*fraction).min(1.0 - 280.0 / dock_width);
        }
        let mut viewer = DiffCompTabViewer {
            state: &mut self.state,
            textures: &mut self.textures,
        };

        DockArea::new(&mut self.dock_state)
            .style(crate::theme::dock_style(ctx))
            .show_close_buttons(false)
            .show_leaf_close_all_buttons(false)
            .show_leaf_collapse_buttons(false)
            .show(ctx, &mut viewer);

        // =====================================================================
        // Dialogs
        // =====================================================================
        panels::show_dialogs(ctx, &mut self.state);

        // =====================================================================
        // Handle diff invalidation from panels (offset/color/blend changes).
        // This must happen AFTER panels run (so values are updated) but
        // BEFORE check_auto_diff (so a fresh computation starts).
        // =====================================================================
        // =====================================================================
        // Handle alignment invalidation (e.g. reference layer changed)
        // =====================================================================
        if self.state.ui.alignment_invalidated {
            self.state.ui.alignment_invalidated = false;
            // Re-run alignment for all target layers against the new reference
            self.perform_alignment();
            // Force a new diff computation
            self.state.ui.diff_invalidated = true;
        }

        if self.state.ui.diff_invalidated {
            self.state.ui.diff_invalidated = false;
            self.state.session.diff_result = None;
            // Bump generation to discard any in-flight computation's result
            self.diff_generation += 1;
            self.state.ui.diff_changed_at = Some(std::time::Instant::now());
            self.state.ui.diff_failed = false;
            self.textures.remove(u64::MAX);
        }

        // =====================================================================
        // Auto-diff: Trigger whenever conditions are met
        // =====================================================================
        self.check_auto_diff();
        if self.state.ui.auto_diff_enabled
            && self.state.session.can_compare()
            && self.state.session.diff_result.is_none()
            && !self.state.ui.diff_failed
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }
}

/// Decode in submission order so a smaller revision cannot become the reference
/// merely because it finished loading before the first document.
#[cfg(not(target_arch = "wasm32"))]
fn spawn_loading(work: impl FnOnce() + Send + 'static) {
    type Job = Box<dyn FnOnce() + Send>;
    static QUEUE: std::sync::OnceLock<std::sync::mpsc::Sender<Job>> = std::sync::OnceLock::new();
    let queue = QUEUE.get_or_init(|| {
        let (sender, receiver) = std::sync::mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("diffcomp-loader".into())
            .spawn(move || {
                for job in receiver {
                    job();
                }
            })
            .expect("Cannot create document loader");
        sender
    });
    queue.send(Box::new(work)).expect("Document loader stopped");
}

/// Keep large document jobs bounded, including imports across project tabs.
#[cfg(not(target_arch = "wasm32"))]
fn spawn_processing(work: impl FnOnce() + Send + 'static) {
    static POOL: std::sync::OnceLock<rayon::ThreadPool> = std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .thread_name(|i| format!("diffcomp-worker-{i}"))
            .build()
            .expect("Cannot create document worker pool")
    })
    .spawn(work);
}

/// Configure the egui visual style.
fn configure_style(ctx: &Context) {
    // ── Load a system font as fallback for symbols & emoji ───────────────
    configure_fonts(ctx);

    crate::theme::configure(ctx);
}

/// Try to load a system font with broad Unicode coverage as a lowest-priority
/// fallback for the Proportional family. This ensures geometric shapes, arrows,
/// and miscellaneous symbols render correctly instead of showing as boxes.
fn configure_fonts(ctx: &Context) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use egui::epaint::text::{FontInsert, FontPriority, InsertFontFamily};

        let candidates: &[&str] = if cfg!(target_os = "macos") {
            &[
                "/System/Library/Fonts/Apple Symbols.ttf",
                "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
            ]
        } else if cfg!(target_os = "windows") {
            &[
                "C:\\Windows\\Fonts\\seguisym.ttf",
                "C:\\Windows\\Fonts\\segoeui.ttf",
            ]
        } else {
            // Linux / BSD
            &[
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
                "/usr/share/fonts/truetype/noto/NotoSansSymbols2-Regular.ttf",
                "/usr/share/fonts/truetype/freefont/FreeSans.ttf",
            ]
        };

        for path in candidates {
            if let Ok(font_bytes) = std::fs::read(path) {
                ctx.add_font(FontInsert::new(
                    "system-symbols",
                    egui::FontData::from_owned(font_bytes),
                    vec![InsertFontFamily {
                        family: egui::FontFamily::Proportional,
                        priority: FontPriority::Lowest,
                    }],
                ));
                break;
            }
        }
    }
}

/// Maximum texture dimension for GPU (conservative limit)
const MAX_TEXTURE_SIZE: u32 = 4096;

/// A single tile of a larger image
pub struct TextureTile {
    texture: egui::TextureHandle,
    /// Position in image coordinates (top-left corner)
    pub x: u32,
    pub y: u32,
    /// Size of this tile
    pub width: u32,
    pub height: u32,
}

impl TextureTile {
    /// Get the texture handle for this tile
    pub fn texture(&self) -> &egui::TextureHandle {
        &self.texture
    }
}

/// Tiled texture for a single layer (handles images larger than GPU limits)
pub struct TiledTexture {
    tiles: Vec<TextureTile>,
    /// Full image dimensions
    #[allow(dead_code)]
    pub full_width: u32,
    #[allow(dead_code)]
    pub full_height: u32,
}

impl TiledTexture {
    /// Create a tiled texture from an image, splitting into GPU-sized chunks
    pub fn from_image(ctx: &egui::Context, layer_id: u64, image: &dc_core::RasterBuffer) -> Self {
        let (full_width, full_height) = image.dimensions();
        let mut tiles = Vec::new();

        // Calculate number of tiles needed
        let tiles_x = (full_width + MAX_TEXTURE_SIZE - 1) / MAX_TEXTURE_SIZE;
        let tiles_y = (full_height + MAX_TEXTURE_SIZE - 1) / MAX_TEXTURE_SIZE;

        for ty in 0..tiles_y {
            for tx in 0..tiles_x {
                let x = tx * MAX_TEXTURE_SIZE;
                let y = ty * MAX_TEXTURE_SIZE;
                let tile_width = (full_width - x).min(MAX_TEXTURE_SIZE);
                let tile_height = (full_height - y).min(MAX_TEXTURE_SIZE);

                // Extract tile pixels
                let mut pixels = Vec::with_capacity((tile_width * tile_height) as usize);
                for py in 0..tile_height {
                    for px in 0..tile_width {
                        let img_x = x + px;
                        let img_y = y + py;
                        let pixel = image.image.get_pixel(img_x, img_y);
                        pixels.push(egui::Color32::from_rgba_unmultiplied(
                            pixel[0], pixel[1], pixel[2], pixel[3],
                        ));
                    }
                }

                let color_image = egui::ColorImage {
                    size: [tile_width as usize, tile_height as usize],
                    pixels,
                };

                let texture = ctx.load_texture(
                    format!("layer_{}_tile_{}_{}", layer_id, tx, ty),
                    color_image,
                    egui::TextureOptions::LINEAR,
                );

                tiles.push(TextureTile {
                    texture,
                    x,
                    y,
                    width: tile_width,
                    height: tile_height,
                });
            }
        }

        Self {
            tiles,
            full_width,
            full_height,
        }
    }

    /// Get all tiles (for rendering)
    pub fn tiles(&self) -> &[TextureTile] {
        &self.tiles
    }
}

/// Cache for egui textures (to avoid re-uploading every frame).
pub struct TextureCache {
    /// Cached tiled textures keyed by layer ID
    textures: std::collections::HashMap<u64, TiledTexture>,
    /// Textures scheduled for deletion (kept alive until next frame)
    garbage: Vec<TiledTexture>,
    thumbnails: std::collections::HashMap<u64, egui::TextureHandle>,
    thumbnail_garbage: Vec<egui::TextureHandle>,
}

impl TextureCache {
    /// Create a new texture cache.
    pub fn new() -> Self {
        Self {
            textures: std::collections::HashMap::new(),
            garbage: Vec::new(),
            thumbnails: std::collections::HashMap::new(),
            thumbnail_garbage: Vec::new(),
        }
    }

    /// Get or create a tiled texture for a layer.
    /// Returns None if the image data is empty.
    pub fn get_or_create(
        &mut self,
        ctx: &egui::Context,
        layer_id: u64,
        image: &dc_core::RasterBuffer,
    ) -> Option<&TiledTexture> {
        // If texture already exists, return it
        if self.textures.contains_key(&layer_id) {
            return self.textures.get(&layer_id);
        }

        // Upload new tiled texture
        let (width, height) = image.dimensions();
        if width == 0 || height == 0 {
            return None;
        }

        let tiled = TiledTexture::from_image(ctx, layer_id, image);
        self.textures.insert(layer_id, tiled);
        self.textures.get(&layer_id)
    }

    /// Small whole-page previews avoid uploading full drawings just to list layers.
    pub fn thumbnail(
        &mut self,
        ctx: &Context,
        id: u64,
        image: &dc_core::RasterBuffer,
    ) -> Option<&egui::TextureHandle> {
        if image.is_empty() {
            return None;
        }
        self.thumbnails.entry(id).or_insert_with(|| {
            let preview = image::imageops::thumbnail(&image.image, 96, 96);
            ctx.load_texture(
                format!("thumbnail-{id}"),
                egui::ColorImage::from_rgba_unmultiplied(
                    [preview.width() as usize, preview.height() as usize],
                    preview.as_raw(),
                ),
                egui::TextureOptions::LINEAR,
            )
        });
        self.thumbnails.get(&id)
    }

    /// Remove a texture from the cache.
    ///
    /// The texture is moved to garbage and kept alive until `process_garbage` is called,
    /// preventing "use after free" panics if the texture was rendered this frame.
    pub fn remove(&mut self, layer_id: u64) {
        if let Some(texture) = self.thumbnails.remove(&layer_id) {
            self.thumbnail_garbage.push(texture);
        }
        if let Some(tex) = self.textures.remove(&layer_id) {
            self.garbage.push(tex);
        }
    }

    /// Clear all cached textures.
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.garbage.extend(self.textures.drain().map(|(_, v)| v));
        self.thumbnail_garbage
            .extend(self.thumbnails.drain().map(|(_, v)| v));
    }

    /// Free garbage textures. Should be called at the start of the frame.
    pub fn process_garbage(&mut self) {
        self.garbage.clear();
        self.thumbnail_garbage.clear();
    }
}

impl Default for TextureCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod regression_tests {
    use super::*;
    fn app() -> DiffCompApp {
        let (tx, rx) = std::sync::mpsc::channel();
        DiffCompApp {
            current_project_id: 0,
            current_project_name: "Test".into(),
            state: AppState::new(None),
            textures: TextureCache::new(),
            dock_state: docking::default_layout(),
            rx,
            tx,
            diff_generation: 1,
            egui_ctx: None,
            other_projects: vec![],
            next_project_id: 1,
            instance_id: "test".into(),
            ipc_inbox_dir: std::env::temp_dir(),
            last_ipc_poll: std::time::Instant::now(),
            active_drag_offer_id: None,
            active_drag_project_id: None,
            pending_spawn_project_id: None,
            pending_spawn_due_ms: None,
        }
    }
    fn image() -> dc_core::RasterBuffer {
        dc_core::RasterBuffer::new(
            image::RgbaImage::from_pixel(4, 4, image::Rgba([255; 4])),
            300,
        )
    }
    #[test]
    fn inactive_project_receives_its_result_without_disturbing_active_project() {
        let mut app = app();
        let mut inactive = AppState::new(None);
        inactive.ui.diff_running = Some(7);
        inactive.ui.is_loading = true;
        app.other_projects.push(ProjectWorkspace {
            id: 2,
            name: "Other".into(),
            state: inactive,
            textures: TextureCache::new(),
            dock_state: docking::default_layout(),
            diff_generation: 7,
        });
        app.tx
            .send(AppMessage::DiffComputed {
                project_id: 2,
                generation: 7,
                result: image(),
            })
            .unwrap();
        app.check_messages();
        assert!(app.other_projects[0].state.session.diff_result.is_some());
        assert!(!app.other_projects[0].state.ui.is_loading);
        assert!(app.state.session.diff_result.is_none());
        assert_eq!(app.current_project_id, 0);
    }
    #[test]
    fn stale_completions_cannot_clear_a_newer_result_or_worker() {
        let mut app = app();
        app.diff_generation = 3;
        app.state.ui.diff_running = Some(3);
        app.state.session.diff_result = Some(image());
        app.state.ui.is_loading = true;
        app.tx
            .send(AppMessage::DiffComputed {
                project_id: 0,
                generation: 2,
                result: image(),
            })
            .unwrap();
        app.tx
            .send(AppMessage::DiffFailed {
                project_id: 0,
                generation: 2,
                error: "old error".into(),
            })
            .unwrap();
        app.check_messages();
        assert!(app.state.session.diff_result.is_some());
        assert_eq!(app.state.ui.diff_running, Some(3));
        assert!(app.state.ui.is_loading);
        assert!(!app.state.ui.diff_failed);
    }
    #[test]
    fn invalidated_generation_finishes_without_publishing_or_spawning_duplicate() {
        let mut app = app();
        app.state.ui.diff_running = Some(1);
        app.state.ui.is_loading = true;
        app.diff_generation = 2;
        app.compute_diff();
        assert_eq!(app.diff_generation, 2);
        app.tx
            .send(AppMessage::DiffComputed {
                project_id: 0,
                generation: 1,
                result: image(),
            })
            .unwrap();
        app.check_messages();
        assert_eq!(app.state.ui.diff_running, None);
        assert!(!app.state.ui.is_loading);
        assert!(app.state.session.diff_result.is_none());
    }
    #[test]
    fn documents_finish_in_original_tab_after_switching() {
        let mut app = app();
        let mut inactive = AppState::new(None);
        inactive.ui.pending_operations = 1;
        inactive.ui.is_loading = true;
        app.other_projects.push(ProjectWorkspace {
            id: 2,
            name: "Other".into(),
            state: inactive,
            textures: TextureCache::new(),
            dock_state: docking::default_layout(),
            diff_generation: 0,
        });
        app.tx
            .send(AppMessage::DocumentsLoaded {
                project_id: 2,
                path: "test.png".into(),
                result: Ok(vec![("test".into(), image())]),
            })
            .unwrap();
        app.check_messages();
        assert_eq!(app.current_project_id, 0);
        assert!(app.state.session.layers.is_empty());
        assert_eq!(app.other_projects[0].state.session.layers.len(), 1);
        assert_eq!(app.other_projects[0].state.ui.pending_operations, 0);
        assert!(!app.other_projects[0].state.ui.is_loading);
    }
    #[test]
    fn late_alignment_after_replacing_source_is_discarded() {
        let mut app = app();
        let reference_id = app
            .state
            .session
            .add_layer("ref".into(), "ref.png".into(), image())
            .unwrap();
        let layer_id = app
            .state
            .session
            .add_layer("target".into(), "target.png".into(), image())
            .unwrap();
        let reference = app
            .state
            .session
            .get_layer(reference_id)
            .unwrap()
            .original
            .clone();
        let target = app
            .state
            .session
            .get_layer(layer_id)
            .unwrap()
            .original
            .clone();
        app.state.session.get_layer_mut(layer_id).unwrap().original = std::sync::Arc::new(image());
        app.state.ui.pending_operations = 1;
        app.tx
            .send(AppMessage::AlignmentComputed {
                project_id: 0,
                layer_id,
                reference_id,
                reference,
                target,
                result: dc_core::force_fit_align(&image(), &image(), true),
            })
            .unwrap();
        app.check_messages();
        assert!(app
            .state
            .session
            .get_layer(layer_id)
            .unwrap()
            .aligned
            .is_none());
        assert_eq!(app.state.ui.pending_operations, 0);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod import_order_tests {
    #[test]
    fn imports_preserve_submission_order() {
        let (tx, rx) = std::sync::mpsc::channel();
        for index in 0..3 {
            let tx = tx.clone();
            super::spawn_loading(move || {
                if index == 0 {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                tx.send(index).unwrap();
            });
        }
        let actual: Vec<_> = (0..3)
            .map(|_| rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap())
            .collect();
        assert_eq!(actual, vec![0, 1, 2]);
    }
}
