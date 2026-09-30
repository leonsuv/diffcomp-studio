// =============================================================================
// dc_app/panels - UI Panel Implementations
// =============================================================================
// Individual panel implementations for the application layout.
// =============================================================================

mod dialogs;
mod layer_panel;
mod legend_panel;
mod markups_list;
mod minimap;
mod properties_panel;
mod render_view;
pub(crate) mod tool_rail;
pub use tool_rail::tool_rail;

pub use dialogs::show_dialogs;
pub use layer_panel::layer_panel;
pub use legend_panel::{is_annotation_flashing, legend_panel, LegendPanelState};
pub use markups_list::{markups_list_panel, MarkupsListState};
pub use minimap::minimap_panel;
pub use properties_panel::properties_panel;
pub use render_view::render_view;
