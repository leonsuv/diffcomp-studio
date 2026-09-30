// =============================================================================
// dc_app/docking - Docking System Implementation
// =============================================================================
// Manages the flexible panel layout using egui_dock.
// =============================================================================

use crate::app::TextureCache;
use crate::panels;
use crate::state::AppState;
use egui::Ui;
use egui_dock::{DockState, NodeIndex, TabViewer};

/// The various types of tabs available in the application.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Tab {
    /// The main document viewport (infinite canvas).
    Viewport(u32),
    /// Layer management panel (left side usually).
    Layers,
    /// Property inspector (right side usually).
    Properties,
    /// Data grid for markups (bottom usually).
    Markups,
    /// Minimap / overview panel.
    Minimap,
    /// Dynamic Legend panel.
    Legend,
}

/// The tab viewer implementation that renders the content of each tab.
pub struct DiffCompTabViewer<'a> {
    pub state: &'a mut AppState,
    pub textures: &'a mut TextureCache,
}

impl<'a> TabViewer for DiffCompTabViewer<'a> {
    type Tab = Tab;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        match tab {
            Tab::Viewport(id) => {
                if *id == 0 {
                    "Document".into()
                } else {
                    format!("Document {}", *id + 1).into()
                }
            }
            Tab::Layers => crate::i18n::tr("tab.layers").into(),
            Tab::Properties => crate::i18n::tr("tab.properties").into(),
            Tab::Markups => crate::i18n::tr("tab.markups").into(),
            Tab::Minimap => crate::i18n::tr("tab.minimap").into(),
            Tab::Legend => crate::i18n::tr("tab.legend").into(),
        }
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Self::Tab) {
        match tab {
            Tab::Viewport(id) => {
                // The viewport needs to fill its container
                ui.push_id(("viewport_tab", *id), |ui| {
                    panels::render_view(ui, self.state, self.textures);
                });
            }
            Tab::Layers => {
                panels::layer_panel(ui, self.state, self.textures);
            }
            Tab::Properties => {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| panels::properties_panel(ui, self.state));
            }
            Tab::Markups => {
                panels::markups_list_panel(ui, self.state);
            }
            Tab::Minimap => {
                panels::minimap_panel(ui, self.state, self.textures);
            }
            Tab::Legend => {
                panels::legend_panel(ui, self.state);
            }
        }
    }
}

/// Keep the document central and group inspectors in a compact right column.
pub fn default_layout() -> DockState<Tab> {
    let mut dock_state = DockState::new(vec![Tab::Viewport(0)]);
    let surface = dock_state.main_surface_mut();
    let [_canvas, inspector] = surface.split_right(NodeIndex::root(), 0.78, vec![Tab::Layers]);
    surface.split_below(inspector, 0.42, vec![Tab::Properties]);
    dock_state
}
