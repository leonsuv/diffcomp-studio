// =============================================================================
// dc_app/i18n - Internationalization
// =============================================================================
// Provides translated UI strings for English and German.
// Usage: `t!(state, key)` for simple strings, `t!(state, key, args...)` for format strings.
// =============================================================================

use std::sync::atomic::{AtomicU8, Ordering};

/// Supported languages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum Language {
    English = 0,
    German = 1,
}

impl Language {
    pub fn label(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::German => "Deutsch",
        }
    }

    pub fn all() -> &'static [Language] {
        &[Language::English, Language::German]
    }
}

impl Default for Language {
    fn default() -> Self {
        Language::German
    }
}

/// Global language setting (atomic for lock-free access from any thread).
static CURRENT_LANGUAGE: AtomicU8 = AtomicU8::new(Language::German as u8);

pub fn set_language(lang: Language) {
    CURRENT_LANGUAGE.store(lang as u8, Ordering::Relaxed);
}

pub fn current_language() -> Language {
    match CURRENT_LANGUAGE.load(Ordering::Relaxed) {
        1 => Language::German,
        _ => Language::English,
    }
}

/// Look up a translated string by key. Returns the English fallback if no translation exists.
pub fn tr(key: &str) -> &'static str {
    let lang = current_language();
    match lang {
        Language::English => en(key),
        Language::German => de(key).unwrap_or_else(|| en(key)),
    }
}

// =============================================================================
// English strings (authoritative / fallback)
// =============================================================================
fn en(key: &str) -> &'static str {
    match key {
        "settings.general" => "General",
        "settings.language" => "Language",
        "settings.processing" => "Comparison processing",
        "settings.automatic" => "Automatic",
        "settings.gpu" => "Graphics processor",
        "settings.cpu" => "Processor",
        "inspector.selection" => "Selection",
        "inspector.comparison" => "Comparison",
        "inspector.document" => "Document",
        "inspector.appearance" => "Appearance",
        "inspector.empty_title" => "Nothing selected",
        "inspector.empty_hint" => {
            "Choose a document, markup or drawing tool to edit its properties."
        }
        "inspector.highlight" => "Highlight",
        "inspector.adjust_position" => "Adjust position…",
        "inspector.position" => "Position",
        "inspector.horizontal" => "Horizontal",
        "inspector.vertical" => "Vertical",
        "inspector.sensitivity" => "Threshold",
        "inspector.colors" => "Comparison colors",
        "inspector.mode_hint" => "Change the view using the comparison toolbar.",
        "inspector.segment" => "Segment",
        "inspector.total" => "Total",
        "inspector.more_tools" => "More tools",
        "workspace.open" => "Open documents",
        "workspace.align" => "Align",
        "workspace.compare" => "Compare",
        "workspace.comparing" => "Comparing…",
        "workspace.fit" => "Fit",
        "workspace.workspace" => "Workspace",
        "workspace.empty_title" => "Compare documents",
        "workspace.empty_hint" => "Open a reference and a revision to highlight changes.",
        "workspace.drop_hint" => "or drop PDF, PNG, JPEG, TIFF or BMP files here",
        "workspace.documents" => "DOCUMENTS",
        "workspace.reference" => "Reference",
        "workspace.revision" => "Revision",
        "workspace.aligned" => "Aligned",
        "workspace.unaligned" => "Not aligned",
        "workspace.revision_colors" => "Set revision colors in the selected layer.",
        "workspace.add" => "+ Add",
        "workspace.visible" => "Visible",
        "workspace.markups" => "Markups",
        "workspace.tolerance" => "Ignore shifts up to 1 pixel",
        // ── Menu Bar ─────────────────────────────────────────────────────
        "menu.file" => "File",
        "menu.edit" => "Edit",
        "menu.view" => "View",
        "menu.compare" => "Compare",
        "menu.tools" => "Tools",
        "menu.help" => "Help",

        // File menu
        "file.open" => "Open...  (Ctrl+O)",
        "file.import_image" => "Import Image...",
        "file.open_session" => "Open Session...",
        "file.save_session" => "Save Session  (Ctrl+S)",
        "file.save_session_as" => "Save Session As...",
        "file.export" => "Export",
        "file.export_flattened" => "Export Flattened Image (PNG)...",
        "file.export_diff" => "Export Diff Image (PNG)...",
        "file.export_layer" => "Export Selected Layer (PNG)...",
        "file.export_csv" => "Export Markups to CSV...",
        "file.export_xlsx" => "Export Markups to Excel (.xlsx)...",
        "file.settings" => "Settings...",
        "file.exit" => "Exit",

        // Edit menu
        "edit.undo" => "Undo  (Ctrl+Z)",
        "edit.redo" => "Redo  (Ctrl+Shift+Z)",

        // View menu
        "view.layer_panel" => "Layer Panel",
        "view.properties_panel" => "Properties Panel",
        "view.markups_list" => "Markups List",
        "view.minimap" => "Minimap",
        "view.legend" => "Legend",
        "view.split_viewport" => "Split Viewport (Horizontal)",
        "view.zoom_in" => "Zoom In  (+)",
        "view.zoom_out" => "Zoom Out  (-)",
        "view.zoom_100" => "Zoom 100%  (1)",
        "view.fit_to_window" => "Fit to Window  (F)",
        "view.reset_view" => "Reset View  (R)",

        // Compare menu
        "compare.auto_diff" => "Auto Diff on Load",
        "compare.align" => "Align Layers",
        "compare.compute_diff" => "Compute Diff",
        "compare.clear_diff" => "Clear Diff",

        // Tools menu
        "tools.calibration" => "Calibration",
        "tools.pixels_per_unit" => "Pixels per unit:",
        "tools.unit" => "Unit:",
        "tools.reset_pixels" => "Reset to pixels",
        "tools.count_counter" => "Count counter:",
        "tools.reset_counter" => "Reset counter to 1",

        // Help menu
        "help.about" => "About",
        "help.license" => "License...",

        // Top-right bar
        "bar.mode" => "Mode:",
        "bar.licensed_to" => "Licensed to:",
        "bar.unlicensed" => "Unlicensed",

        // Status bar
        "status.ready" => "Ready",
        "status.layers" => "Layers:",
        "status.zoom" => "Zoom:",

        // ── Status Messages ──────────────────────────────────────────────
        "status.annotation_deleted" => "Annotation deleted",
        "status.layer_deleted" => "Layer deleted",
        "status.select_mode" => "Select Mode",
        "status.selection_cleared" => "Selection cleared",
        "status.tool_cancelled" => "Tool cancelled",
        "status.nothing_to_undo" => "Nothing to undo",
        "status.nothing_to_redo" => "Nothing to redo",
        "status.session_loaded" => "Session loaded successfully",
        "status.session_saved" => "Session saved successfully",
        "status.session_load_web" => "Session loading not supported on web yet",
        "status.session_save_web" => "Session saving not supported on web yet",
        "status.diff_complete" => "Comparison ready",
        "status.aligning" => "Aligning layers...",
        "status.alignment_complete" => "Alignment complete",
        "status.need_2_layers" => "Need at least 2 layers to align",
        "status.no_ref_layer" => "No reference layer found",
        "status.no_visible_ref" => "No visible reference layer",
        "status.no_visible_targets" => "No visible target layers to compare",
        "status.computing_diff" => "Computing diff (morphological)...",
        "status.slipsheet" => "Slip-sheeting...",
        "status.slipsheet_fail" => "Failed to load new file for slip-sheeting",
        "status.no_layers_export" => "No visible layers to export",
        "status.no_diff_export" => "No diff result to export",
        "status.no_layer_export" => "No layer selected to export",
        "status.csv_exported" => "CSV Exported successfully.",
        "status.xlsx_exported" => "Excel (.xlsx) Exported successfully.",
        "status.hwid_copied" => "Hardware ID copied to clipboard",

        // ── Tab Titles ───────────────────────────────────────────────────
        "tab.viewport" => "Viewport",
        "tab.layers" => "Layers",
        "tab.properties" => "Inspector",
        "tab.markups" => "Markups",
        "tab.minimap" => "Minimap",
        "tab.legend" => "Legend",

        // ── Layer Panel ──────────────────────────────────────────────────
        "layers.heading" => "Layers",
        "layers.add" => "+ Add",
        "layers.snapshot" => "Snapshot",
        "layers.snapshot_tip" => "Create a new layer from current visible view",
        "layers.flatten" => "Flatten",
        "layers.flatten_tip" => "Merge all visible layers into one",
        "layers.no_layers" => "No layers loaded",
        "layers.drop_hint" => "Drag & drop files here\nor use File > Open",
        "layers.toggle_vis" => "Toggle visibility",
        "layers.ref_layer" => "Reference layer",
        "layers.aligned" => "Aligned",
        "layers.low_confidence" => "Low confidence",
        "layers.align_failed" => "[x] Alignment failed",
        "layers.not_aligned" => "[ ] Not aligned",
        "layers.set_ref" => "Set as Reference",
        "layers.slipsheet_page" => "Slip-Sheet Page...",
        "layers.slipsheet_doc" => "Slip-Sheet Document...",
        "layers.remove" => "Remove",
        "layers.up" => "Up",
        "layers.down" => "Down",
        "layers.remove_btn" => "Remove",

        // ── Markups List ─────────────────────────────────────────────────
        "markups.filter_type" => "Filter by type…",
        "markups.all_statuses" => "All Statuses",
        "markups.markups" => "markups",
        "markups.columns" => "Columns",
        "markups.add_column" => "+ Add Column",
        "markups.export" => "Export",
        "markups.export_csv" => "Export to CSV",
        "markups.export_xlsx" => "Export to Excel (.xlsx)",
        "markups.clear_status" => "— Clear Status",
        "markups.delete" => "Delete",
        "markups.add_col_title" => "Add Custom Column",
        "markups.col_name" => "Name:",
        "markups.col_type" => "Type:",
        "markups.formula_label" => "Formula:",
        "markups.formula_hint" => "e.g. [Area] * 4.50. You can use standard math operations.",
        "markups.cancel" => "Cancel",
        "markups.add" => "Add",

        // Table headers
        "table.type" => "Type",
        "table.layer" => "Layer",
        "table.color" => "Color",
        "table.opacity" => "Opacity",
        "table.width" => "Width",
        "table.position" => "Position",
        "table.size" => "Size",
        "table.status" => "Status",

        // Column types
        "coltype.text" => "Text",
        "coltype.number" => "Number",
        "coltype.formula" => "Formula",

        // Annotation type names
        "annot.path" => "Path",
        "annot.rectangle" => "Rectangle",
        "annot.ellipse" => "Ellipse",
        "annot.cloud" => "Cloud",
        "annot.line" => "Line",
        "annot.text" => "Text",
        "annot.area" => "Area",
        "annot.measurement" => "Measurement",
        "annot.count" => "Count",
        "annot.viewport" => "Viewport",
        "annot.dim_chain" => "Dim Chain",

        // ── Properties Panel ─────────────────────────────────────────────
        "props.heading" => "Properties",
        "props.annot_props" => "Annotation Properties",
        "props.color" => "Color:",
        "props.line_width" => "Line Width:",
        "props.opacity" => "Opacity:",
        "props.font_size" => "Font Size:",
        "props.content" => "Content:",
        "props.line_endings" => "Line Endings",
        "props.start" => "Start:",
        "props.end" => "End:",
        "props.punch_list" => "Punch List",
        "props.number" => "Number:",
        "props.seq_group" => "Seq Group:",
        "props.none" => "(None)",
        "props.new_group" => "New Group:",
        "props.new_group_hint" => "e.g. 'Electrical'",
        "props.viewport_props" => "Viewport Properties",
        "props.scale" => "Scale (px/unit):",
        "props.quick_scale" => "Quick Scale:",
        "props.label" => "Label:",
        "props.dimension_chain" => "Dimension Chain",
        "props.fill" => "Fill",
        "props.deselect_tool" => "Deselect Tool",
        "props.layer_props" => "Layer Properties",
        "props.name" => "Name:",
        "props.visible" => "Visible",
        "props.blend_color" => "Blend Color:",
        "props.manual_transform" => "Manual Transform",
        "props.offset_x" => "Offset X:",
        "props.offset_y" => "Offset Y:",
        "props.no_layer" => "No layer selected",
        "props.diff_settings" => "Diff Settings",
        "props.blend_mode" => "Blend Mode:",
        "props.overlay_opacity" => "Overlay Opacity:",
        "props.threshold" => "Threshold:",
        "props.noise_filter" => "Noise Filter:",
        "props.ref_color" => "Reference Color:",
        "props.target_color" => "Target Color:",
        "props.reset_view" => "Reset View",
        "props.fit_to_content" => "Fit to Content",

        // Blend modes
        "blend.overlay" => "Overlay",
        "blend.color_diff" => "Color Difference",
        "blend.heatmap" => "Heatmap",
        "blend.binary_mask" => "Binary Mask",
        "blend.subtract" => "Subtract",
        "blend.xor" => "XOR",

        // ── Drawing tools ───────────────────────────────────────────────────
        "tools.mode" => "Mode:",
        "tools.pan" => "Pan",
        "tools.select" => "Select",
        "tools.defaults" => "+ Defaults",
        "tools.punch_group" => "Punch List Group",
        "tools.none_global" => "(None — global counter)",
        "tools.new_group_hint" => "New group…",
        "tools.mep_rise_drop" => "MEP Rise/Drop",
        "tools.mep" => "MEP",

        // ── Render View ──────────────────────────────────────────────────
        "render.drop_hint" => "Drop files here to compare",
        "render.formats" => "Supports PDF, PNG, JPEG, TIFF, BMP",
        "render.or_open" => "Or use File > Open (Ctrl+O)",
        "render.all_hidden" => "All layers hidden",
        "render.mode_pan" => "Pan",
        "render.mode_zoom" => "Zoom",
        "render.mode_measure" => "Measure",
        "render.mode_select" => "Select",
        "render.mode_draw" => "Draw",

        // ── Minimap ──────────────────────────────────────────────────────
        "minimap.no_doc" => "No document loaded",

        // ── Legend Panel ─────────────────────────────────────────────────
        "legend.heading" => "Legend",
        "legend.subject" => "Subject",
        "legend.type" => "Type",
        "legend.group_by" => "Group by:",
        "legend.counts" => "Counts",
        "legend.color" => "Color",
        "legend.filter" => "Filter...",
        "legend.no_annots" => "No annotations to display.",
        "legend.symbol" => "Symbol",
        "legend.count" => "Count",
        "legend.value" => "Value",
        "legend.types" => "Types",
        "legend.unknown_layer" => "  Unknown layer",

        // ── Dialogs ──────────────────────────────────────────────────────
        "dlg.flatten_title" => "Flatten Layers",
        "dlg.flatten_heading" => "Confirm Flatten",
        "dlg.flatten_desc1" => "This will merge all visible layers into a single image.",
        "dlg.flatten_desc2" => "Original layers will be removed.",
        "dlg.flatten_desc3" => "What would you like to do with existing annotations?",
        "dlg.keep_merge" => "Keep & Merge",
        "dlg.keep_merge_tip" => "Preserve annotations on the new flattened layer",
        "dlg.discard" => "Discard",
        "dlg.discard_tip" => "Remove annotations",
        "dlg.cancel" => "Cancel",
        "dlg.about_title" => "About DiffComp Studio",
        "dlg.about_heading" => "DiffComp Studio",
        "dlg.about_desc1" => "Engineering-grade document comparison tool",
        "dlg.about_desc2" => "with pixel-perfect alignment technology.",
        "dlg.tech_stack" => "Technology Stack:",
        "dlg.copyright" => "© 2024-2026 DiffComp Studio Team",
        "dlg.rights" => "All rights reserved.",
        "dlg.license_title" => "License Management",
        "dlg.license_status" => "License Status",
        "dlg.valid_license" => "Valid License",
        "dlg.invalid_license" => "License Invalid",
        "dlg.no_license" => "No license installed",
        "dlg.perpetual" => "Perpetual license",
        "dlg.licensed_features" => "Licensed features:",
        "dlg.hwid_title" => "Hardware ID",
        "dlg.hwid_desc" => "Your machine's Hardware ID (provide this when purchasing):",
        "dlg.hwid_copy" => "Copy",
        "dlg.hwid_fail" => "Unable to generate Hardware ID",
        "dlg.enter_key" => "Enter License Key",
        "dlg.paste_key" => "Paste your license key below:",
        "dlg.verify" => "Verify License",
        "dlg.clear" => "Clear",
        "dlg.settings_title" => "Settings",
        "dlg.settings_heading" => "Application Settings",
        "dlg.workflow_states" => "Workflow States",
        "dlg.workflow_desc" => "Define custom workflow states for markups.",
        "dlg.add_state" => "+ Add State",
        "dlg.new_state" => "New State",
        "dlg.performance" => "Performance",

        // ── File Dialog Filters ──────────────────────────────────────────
        "filter.supported" => "Supported Documents",
        "filter.pdf" => "PDF Documents",
        "filter.images" => "Images",
        "filter.session" => "DiffComp Session",
        "filter.png" => "PNG Image",
        "filter.csv" => "CSV File",
        "filter.xlsx" => "Excel Document",

        // ── Misc ─────────────────────────────────────────────────────────
        "misc.unknown" => "Unknown",
        "misc.flattened" => "Flattened",
        "misc.snapshot" => "Snapshot",

        // Language selector
        "lang.label" => "Lang:",

        _ => "???", // Unknown key
    }
}

// =============================================================================
// German translations
// =============================================================================
fn de(key: &str) -> Option<&'static str> {
    Some(match key {
        "settings.general" => "Allgemein",
        "settings.language" => "Sprache",
        "settings.processing" => "Vergleichsberechnung",
        "settings.automatic" => "Automatisch",
        "settings.gpu" => "Grafikprozessor",
        "settings.cpu" => "Prozessor",
        "inspector.selection" => "Auswahl",
        "inspector.comparison" => "Vergleich",
        "inspector.document" => "Dokument",
        "inspector.appearance" => "Darstellung",
        "inspector.empty_title" => "Keine Auswahl",
        "inspector.empty_hint" => {
            "Dokument, Markierung oder Werkzeug auswählen, um Eigenschaften zu bearbeiten."
        }
        "inspector.highlight" => "Markierungsfarbe",
        "inspector.adjust_position" => "Position anpassen…",
        "inspector.position" => "Position",
        "inspector.horizontal" => "Horizontal",
        "inspector.vertical" => "Vertikal",
        "inspector.sensitivity" => "Schwellenwert",
        "inspector.colors" => "Vergleichsfarben",
        "inspector.mode_hint" => "Die Ansicht in der Vergleichsleiste ändern.",
        "inspector.segment" => "Abschnitt",
        "inspector.total" => "Gesamt",
        "inspector.more_tools" => "Weitere Werkzeuge",
        "workspace.open" => "Dokumente öffnen",
        "workspace.align" => "Ausrichten",
        "workspace.compare" => "Vergleichen",
        "workspace.comparing" => "Vergleich läuft…",
        "workspace.fit" => "Einpassen",
        "workspace.workspace" => "Arbeitsbereich",
        "workspace.empty_title" => "Dokumente vergleichen",
        "workspace.empty_hint" => "Referenz und Revision öffnen, um Änderungen hervorzuheben.",
        "workspace.drop_hint" => "oder PDF-, PNG-, JPEG-, TIFF- oder BMP-Dateien hier ablegen",
        "workspace.documents" => "DOKUMENTE",
        "workspace.reference" => "Referenz",
        "workspace.revision" => "Revision",
        "workspace.aligned" => "Ausgerichtet",
        "workspace.unaligned" => "Nicht ausgerichtet",
        "workspace.revision_colors" => "Revisionsfarben in der ausgewählten Ebene einstellen.",
        "workspace.add" => "+ Hinzufügen",
        "workspace.visible" => "Sichtbar",
        "workspace.markups" => "Markierungen",
        "workspace.tolerance" => "Verschiebungen bis 1 Pixel ignorieren",
        // ── Menu Bar ─────────────────────────────────────────────────────
        "menu.file" => "Datei",
        "menu.edit" => "Bearbeiten",
        "menu.view" => "Ansicht",
        "menu.compare" => "Vergleichen",
        "menu.tools" => "Werkzeuge",
        "menu.help" => "Hilfe",

        // File menu
        "file.open" => "Öffnen...  (Strg+O)",
        "file.import_image" => "Bild importieren...",
        "file.open_session" => "Sitzung öffnen...",
        "file.save_session" => "Sitzung speichern  (Strg+S)",
        "file.save_session_as" => "Sitzung speichern unter...",
        "file.export" => "Exportieren",
        "file.export_flattened" => "Zusammengefügtes Bild exportieren (PNG)...",
        "file.export_diff" => "Differenzbild exportieren (PNG)...",
        "file.export_layer" => "Ausgewählte Ebene exportieren (PNG)...",
        "file.export_csv" => "Markierungen als CSV exportieren...",
        "file.export_xlsx" => "Markierungen als Excel (.xlsx) exportieren...",
        "file.settings" => "Einstellungen...",
        "file.exit" => "Beenden",

        // Edit menu
        "edit.undo" => "Rückgängig  (Strg+Z)",
        "edit.redo" => "Wiederholen  (Strg+Shift+Z)",

        // View menu
        "view.layer_panel" => "Ebenen-Panel",
        "view.properties_panel" => "Eigenschaften-Panel",
        "view.markups_list" => "Markierungs-Liste",
        "view.minimap" => "Übersichtskarte",
        "view.legend" => "Legende",
        "view.split_viewport" => "Ansicht teilen (Horizontal)",
        "view.zoom_in" => "Vergrößern  (+)",
        "view.zoom_out" => "Verkleinern  (-)",
        "view.zoom_100" => "Zoom 100%  (1)",
        "view.fit_to_window" => "An Fenster anpassen  (F)",
        "view.reset_view" => "Ansicht zurücksetzen  (R)",

        // Compare menu
        "compare.auto_diff" => "Automatischer Vergleich beim Laden",
        "compare.align" => "Ebenen ausrichten",
        "compare.compute_diff" => "Differenz berechnen",
        "compare.clear_diff" => "Differenz löschen",

        // Tools menu
        "tools.calibration" => "Kalibrierung",
        "tools.pixels_per_unit" => "Pixel pro Einheit:",
        "tools.unit" => "Einheit:",
        "tools.reset_pixels" => "Auf Pixel zurücksetzen",
        "tools.count_counter" => "Zähler:",
        "tools.reset_counter" => "Zähler auf 1 zurücksetzen",

        // Help menu
        "help.about" => "Über",
        "help.license" => "Lizenz...",

        // Top-right bar
        "bar.mode" => "Modus:",
        "bar.licensed_to" => "Lizenziert für:",
        "bar.unlicensed" => "Nicht lizenziert",

        // Status bar
        "status.ready" => "Bereit",
        "status.layers" => "Ebenen:",
        "status.zoom" => "Zoom:",

        // ── Status Messages ──────────────────────────────────────────────
        "status.annotation_deleted" => "Anmerkung gelöscht",
        "status.layer_deleted" => "Ebene gelöscht",
        "status.select_mode" => "Auswahlmodus",
        "status.selection_cleared" => "Auswahl aufgehoben",
        "status.tool_cancelled" => "Werkzeug abgebrochen",
        "status.nothing_to_undo" => "Nichts zum Rückgängig machen",
        "status.nothing_to_redo" => "Nichts zum Wiederholen",
        "status.session_loaded" => "Sitzung erfolgreich geladen",
        "status.session_saved" => "Sitzung erfolgreich gespeichert",
        "status.session_load_web" => "Sitzung laden wird im Web noch nicht unterstützt",
        "status.session_save_web" => "Sitzung speichern wird im Web noch nicht unterstützt",
        "status.diff_complete" => "Vergleich bereit",
        "status.aligning" => "Ebenen werden ausgerichtet...",
        "status.alignment_complete" => "Ausrichtung abgeschlossen",
        "status.need_2_layers" => "Mindestens 2 Ebenen zum Ausrichten erforderlich",
        "status.no_ref_layer" => "Keine Referenzebene gefunden",
        "status.no_visible_ref" => "Keine sichtbare Referenzebene",
        "status.no_visible_targets" => "Keine sichtbaren Vergleichsebenen",
        "status.computing_diff" => "Differenz wird berechnet...",
        "status.slipsheet" => "Seitenersetzung...",
        "status.slipsheet_fail" => "Neue Datei für Seitenersetzung konnte nicht geladen werden",
        "status.no_layers_export" => "Keine sichtbaren Ebenen zum Exportieren",
        "status.no_diff_export" => "Kein Differenzergebnis zum Exportieren",
        "status.no_layer_export" => "Keine Ebene zum Exportieren ausgewählt",
        "status.csv_exported" => "CSV erfolgreich exportiert.",
        "status.xlsx_exported" => "Excel (.xlsx) erfolgreich exportiert.",
        "status.hwid_copied" => "Hardware-ID in die Zwischenablage kopiert",

        // ── Tab Titles ───────────────────────────────────────────────────
        "tab.viewport" => "Ansicht",
        "tab.layers" => "Ebenen",
        "tab.properties" => "Inspector",
        "tab.markups" => "Markierungen",
        "tab.minimap" => "Übersicht",
        "tab.legend" => "Legende",

        // ── Layer Panel ──────────────────────────────────────────────────
        "layers.heading" => "Ebenen",
        "layers.add" => "+ Hinzufügen",
        "layers.snapshot" => "Schnappschuss",
        "layers.snapshot_tip" => "Neue Ebene aus aktueller Ansicht erstellen",
        "layers.flatten" => "Zusammenfügen",
        "layers.flatten_tip" => "Alle sichtbaren Ebenen zusammenfügen",
        "layers.no_layers" => "Keine Ebenen geladen",
        "layers.drop_hint" => "Dateien hier ablegen\noder Datei > Öffnen verwenden",
        "layers.toggle_vis" => "Sichtbarkeit umschalten",
        "layers.ref_layer" => "Referenzebene",
        "layers.aligned" => "Ausgerichtet",
        "layers.low_confidence" => "Geringe Konfidenz",
        "layers.align_failed" => "[x] Ausrichtung fehlgeschlagen",
        "layers.not_aligned" => "[ ] Nicht ausgerichtet",
        "layers.set_ref" => "Als Referenz setzen",
        "layers.slipsheet_page" => "Seite ersetzen...",
        "layers.slipsheet_doc" => "Dokument ersetzen...",
        "layers.remove" => "Entfernen",
        "layers.up" => "Hoch",
        "layers.down" => "Runter",
        "layers.remove_btn" => "Entfernen",

        // ── Markups List ─────────────────────────────────────────────────
        "markups.filter_type" => "Nach Typ filtern…",
        "markups.all_statuses" => "Alle Status",
        "markups.markups" => "Markierungen",
        "markups.columns" => "Spalten",
        "markups.add_column" => "+ Spalte hinzufügen",
        "markups.export" => "Export",
        "markups.export_csv" => "Als CSV exportieren",
        "markups.export_xlsx" => "Als Excel (.xlsx) exportieren",
        "markups.clear_status" => "— Status löschen",
        "markups.delete" => "Löschen",
        "markups.add_col_title" => "Benutzerdefinierte Spalte hinzufügen",
        "markups.col_name" => "Name:",
        "markups.col_type" => "Typ:",
        "markups.formula_label" => "Formel:",
        "markups.formula_hint" => "z.B. [Area] * 4.50. Standard-Rechenoperationen möglich.",
        "markups.cancel" => "Abbrechen",
        "markups.add" => "Hinzufügen",

        // Table headers
        "table.type" => "Typ",
        "table.layer" => "Ebene",
        "table.color" => "Farbe",
        "table.opacity" => "Deckkraft",
        "table.width" => "Breite",
        "table.position" => "Position",
        "table.size" => "Größe",
        "table.status" => "Status",

        // Column types
        "coltype.text" => "Text",
        "coltype.number" => "Zahl",
        "coltype.formula" => "Formel",

        // Annotation type names
        "annot.path" => "Pfad",
        "annot.rectangle" => "Rechteck",
        "annot.ellipse" => "Ellipse",
        "annot.cloud" => "Wolke",
        "annot.line" => "Linie",
        "annot.text" => "Text",
        "annot.area" => "Fläche",
        "annot.measurement" => "Messung",
        "annot.count" => "Zähler",
        "annot.viewport" => "Ansichtsfenster",
        "annot.dim_chain" => "Maßkette",

        // ── Properties Panel ─────────────────────────────────────────────
        "props.heading" => "Eigenschaften",
        "props.annot_props" => "Anmerkungseigenschaften",
        "props.color" => "Farbe:",
        "props.line_width" => "Linienbreite:",
        "props.opacity" => "Deckkraft:",
        "props.font_size" => "Schriftgröße:",
        "props.content" => "Inhalt:",
        "props.line_endings" => "Linienenden",
        "props.start" => "Anfang:",
        "props.end" => "Ende:",
        "props.punch_list" => "Mängelliste",
        "props.number" => "Nummer:",
        "props.seq_group" => "Sequenzgruppe:",
        "props.none" => "(Keine)",
        "props.new_group" => "Neue Gruppe:",
        "props.new_group_hint" => "z.B. 'Elektro'",
        "props.viewport_props" => "Ansichtsfenster-Eigenschaften",
        "props.scale" => "Maßstab (px/Einheit):",
        "props.quick_scale" => "Schnellmaßstab:",
        "props.label" => "Bezeichnung:",
        "props.dimension_chain" => "Maßkette",
        "props.fill" => "Füllung",
        "props.deselect_tool" => "Werkzeug abwählen",
        "props.layer_props" => "Ebeneneigenschaften",
        "props.name" => "Name:",
        "props.visible" => "Sichtbar",
        "props.blend_color" => "Mischfarbe:",
        "props.manual_transform" => "Manuelle Transformation",
        "props.offset_x" => "Versatz X:",
        "props.offset_y" => "Versatz Y:",
        "props.no_layer" => "Keine Ebene ausgewählt",
        "props.diff_settings" => "Differenz-Einstellungen",
        "props.blend_mode" => "Mischmodus:",
        "props.overlay_opacity" => "Überlagerungs-Deckkraft:",
        "props.threshold" => "Schwellenwert:",
        "props.noise_filter" => "Rauschfilter:",
        "props.ref_color" => "Referenzfarbe:",
        "props.target_color" => "Vergleichsfarbe:",
        "props.reset_view" => "Ansicht zurücksetzen",
        "props.fit_to_content" => "An Inhalt anpassen",

        // Blend modes
        "blend.overlay" => "Überlagerung",
        "blend.color_diff" => "Farbunterschied",
        "blend.heatmap" => "Heatmap",
        "blend.binary_mask" => "Binärmaske",
        "blend.subtract" => "Subtrahieren",
        "blend.xor" => "XOR",

        // ── Drawing tools ───────────────────────────────────────────────────
        "tools.mode" => "Modus:",
        "tools.pan" => "Verschieben",
        "tools.select" => "Auswählen",
        "tools.defaults" => "+ Standards",
        "tools.punch_group" => "Mängelliste-Gruppe",
        "tools.none_global" => "(Keine — globaler Zähler)",
        "tools.new_group_hint" => "Neue Gruppe…",
        "tools.mep_rise_drop" => "TGA Steig-/Fallleitung",
        "tools.mep" => "TGA",

        // ── Render View ──────────────────────────────────────────────────
        "render.drop_hint" => "Dateien hier ablegen zum Vergleichen",
        "render.formats" => "Unterstützt PDF, PNG, JPEG, TIFF, BMP",
        "render.or_open" => "Oder Datei > Öffnen (Strg+O)",
        "render.all_hidden" => "Alle Ebenen ausgeblendet",
        "render.mode_pan" => "Verschieben",
        "render.mode_zoom" => "Zoom",
        "render.mode_measure" => "Messen",
        "render.mode_select" => "Auswählen",
        "render.mode_draw" => "Zeichnen",

        // ── Minimap ──────────────────────────────────────────────────────
        "minimap.no_doc" => "Kein Dokument geladen",

        // ── Legend Panel ─────────────────────────────────────────────────
        "legend.heading" => "Legende",
        "legend.subject" => "Betreff",
        "legend.type" => "Typ",
        "legend.group_by" => "Gruppieren nach:",
        "legend.counts" => "Anzahlen",
        "legend.color" => "Farbe",
        "legend.filter" => "Filtern...",
        "legend.no_annots" => "Keine Anmerkungen vorhanden.",
        "legend.symbol" => "Symbol",
        "legend.count" => "Anzahl",
        "legend.value" => "Wert",
        "legend.types" => "Typen",
        "legend.unknown_layer" => "  Unbekannte Ebene",

        // ── Dialogs ──────────────────────────────────────────────────────
        "dlg.flatten_title" => "Ebenen zusammenfügen",
        "dlg.flatten_heading" => "Zusammenfügen bestätigen",
        "dlg.flatten_desc1" => "Alle sichtbaren Ebenen werden zu einem Bild zusammengefügt.",
        "dlg.flatten_desc2" => "Ursprüngliche Ebenen werden entfernt.",
        "dlg.flatten_desc3" => "Was soll mit bestehenden Anmerkungen geschehen?",
        "dlg.keep_merge" => "Behalten & Zusammenfügen",
        "dlg.keep_merge_tip" => "Anmerkungen auf der neuen Ebene beibehalten",
        "dlg.discard" => "Verwerfen",
        "dlg.discard_tip" => "Anmerkungen entfernen",
        "dlg.cancel" => "Abbrechen",
        "dlg.about_title" => "Über DiffComp Studio",
        "dlg.about_heading" => "DiffComp Studio",
        "dlg.about_desc1" => "Professionelles Dokumentenvergleichswerkzeug",
        "dlg.about_desc2" => "mit pixelgenauer Ausrichtungstechnologie.",
        "dlg.tech_stack" => "Technologie-Stack:",
        "dlg.copyright" => "© 2024-2026 DiffComp Studio Team",
        "dlg.rights" => "Alle Rechte vorbehalten.",
        "dlg.license_title" => "Lizenzverwaltung",
        "dlg.license_status" => "Lizenzstatus",
        "dlg.valid_license" => "Gültige Lizenz",
        "dlg.invalid_license" => "Lizenz ungültig",
        "dlg.no_license" => "Keine Lizenz installiert",
        "dlg.perpetual" => "Unbefristete Lizenz",
        "dlg.licensed_features" => "Lizenzierte Funktionen:",
        "dlg.hwid_title" => "Hardware-ID",
        "dlg.hwid_desc" => "Ihre Hardware-ID (beim Kauf angeben):",
        "dlg.hwid_copy" => "Kopieren",
        "dlg.hwid_fail" => "Hardware-ID konnte nicht generiert werden",
        "dlg.enter_key" => "Lizenzschlüssel eingeben",
        "dlg.paste_key" => "Fügen Sie Ihren Lizenzschlüssel unten ein:",
        "dlg.verify" => "Lizenz überprüfen",
        "dlg.clear" => "Löschen",
        "dlg.settings_title" => "Einstellungen",
        "dlg.settings_heading" => "Anwendungseinstellungen",
        "dlg.workflow_states" => "Workflow-Status",
        "dlg.workflow_desc" => "Benutzerdefinierte Workflow-Status für Markierungen festlegen.",
        "dlg.add_state" => "+ Status hinzufügen",
        "dlg.new_state" => "Neuer Status",
        "dlg.performance" => "Leistung",

        // ── File Dialog Filters ──────────────────────────────────────────
        "filter.supported" => "Unterstützte Dokumente",
        "filter.pdf" => "PDF-Dokumente",
        "filter.images" => "Bilder",
        "filter.session" => "DiffComp-Sitzung",
        "filter.png" => "PNG-Bild",
        "filter.csv" => "CSV-Datei",
        "filter.xlsx" => "Excel-Dokument",

        // ── Misc ─────────────────────────────────────────────────────────
        "misc.unknown" => "Unbekannt",
        "misc.flattened" => "Zusammengefügt",
        "misc.snapshot" => "Schnappschuss",

        // Language selector
        "lang.label" => "Sprache:",

        _ => return None,
    })
}
