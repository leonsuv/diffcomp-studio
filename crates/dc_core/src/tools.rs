// =============================================================================
// dc_core/tools - Annotation Tool Data Structures
// =============================================================================
// Defines drawing tool types, styles and line endings.
// =============================================================================

use crate::types::LayerColor;
use serde::{Deserialize, Serialize};
use std::fmt;

/// The fundamental type of annotation to create.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ToolType {
    /// Freehand vector path
    Pen,
    /// Thick translucent line (multiply blend)
    Highlighter,
    /// Line segment
    Line,
    /// Arrow / Leader line
    Arrow,
    /// Geometric rectangle
    Rectangle,
    /// Geometric ellipse
    Ellipse,
    /// Revision cloud
    Cloud,
    /// Text box with leader line
    Callout,
    /// Simple text box
    Text,
    /// Measure a straight-line distance
    MeasureLength,
    /// Measure cumulative distance along a polyline
    MeasurePolylength,
    /// Measure area of a closed polygon
    MeasureArea,
    /// Place a numbered count marker
    Count,
    /// Define a viewport rectangle with local scale
    Viewport,
    /// Continuous dimension chain measurement
    DimensionChain,
}

impl std::fmt::Display for ToolType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pen => write!(f, "Pen"),
            Self::Highlighter => write!(f, "Highlighter"),
            Self::Line => write!(f, "Line"),
            Self::Arrow => write!(f, "Arrow"),
            Self::Rectangle => write!(f, "Rectangle"),
            Self::Ellipse => write!(f, "Ellipse"),
            Self::Cloud => write!(f, "Cloud"),
            Self::Callout => write!(f, "Callout"),
            Self::Text => write!(f, "Text"),
            Self::MeasureLength => write!(f, "Length"),
            Self::MeasurePolylength => write!(f, "Polylength"),
            Self::MeasureArea => write!(f, "Area"),
            Self::Count => write!(f, "Count"),
            Self::Viewport => write!(f, "Viewport"),
            Self::DimensionChain => write!(f, "Dim Chain"),
        }
    }
}

/// Type of symbol rendered at a line ending (start or end).
/// Used for MEP (Mechanical/Electrical/Plumbing) workflows
/// where rise/drop symbols indicate direction of travel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum LineEndingType {
    /// No symbol at this end of the line
    #[default]
    None,
    /// Filled triangle arrowhead pointing along the line direction
    Arrow,
    /// Open (outline only) triangle arrowhead
    OpenArrow,
    /// Closed (filled + outline) triangle arrowhead
    ClosedArrow,
    /// Diamond / lozenge
    Diamond,
    /// Filled circle (dot)
    Circle,
    /// Filled square
    Square,
    /// Rise symbol — upward-pointing triangle with horizontal bar (pipe/duct goes UP)
    RiseSymbol,
    /// Drop symbol — downward-pointing triangle with horizontal bar (pipe/duct goes DOWN)
    DropSymbol,
}

impl fmt::Display for LineEndingType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => write!(f, "None"),
            Self::Arrow => write!(f, "Arrow"),
            Self::OpenArrow => write!(f, "Open Arrow"),
            Self::ClosedArrow => write!(f, "Closed Arrow"),
            Self::Diamond => write!(f, "Diamond"),
            Self::Circle => write!(f, "Circle"),
            Self::Square => write!(f, "Square"),
            Self::RiseSymbol => write!(f, "Rise"),
            Self::DropSymbol => write!(f, "Drop"),
        }
    }
}

impl LineEndingType {
    /// Returns all available line ending types for UI enumeration.
    pub const ALL: &'static [LineEndingType] = &[
        LineEndingType::None,
        LineEndingType::Arrow,
        LineEndingType::OpenArrow,
        LineEndingType::ClosedArrow,
        LineEndingType::Diamond,
        LineEndingType::Circle,
        LineEndingType::Square,
        LineEndingType::RiseSymbol,
        LineEndingType::DropSymbol,
    ];

    /// Returns a short icon/label for compact display.
    pub fn icon(&self) -> &'static str {
        match self {
            Self::None => "\u{2014}",
            Self::Arrow => ">",
            Self::OpenArrow => ">>",
            Self::ClosedArrow => "|>",
            Self::Diamond => "<>",
            Self::Circle => "o",
            Self::Square => "[]",
            Self::RiseSymbol => "^",
            Self::DropSymbol => "v",
        }
    }
}

/// Visual properties for a tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolStyle {
    /// Main stroke color (RGB)
    pub stroke_color: LayerColor,

    /// Fill color (RGB), if applicable
    pub fill_color: Option<LayerColor>,

    /// Opacity (0.0 - 1.0). Applied to both stroke and fill.
    pub opacity: f32,

    /// Line width in points/pixels
    pub line_width: f32,

    /// Font size (for Text/Callout tools)
    pub font_size: f32,

    /// Symbol at the start point of a line/arrow
    #[serde(default)]
    pub line_ending_start: LineEndingType,

    /// Symbol at the end point of a line/arrow
    #[serde(default)]
    pub line_ending_end: LineEndingType,
}

impl Default for ToolStyle {
    fn default() -> Self {
        Self {
            stroke_color: LayerColor::new(255, 0, 0), // Red
            fill_color: None,
            opacity: 1.0,
            line_width: 2.0,
            font_size: 12.0,
            line_ending_start: LineEndingType::None,
            line_ending_end: LineEndingType::None,
        }
    }
}

/// A reusable tool definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tool {
    /// Unique identifier (usually UUID-like)
    pub id: String,

    /// Display name (e.g. "Engineer's Red Pen")
    pub name: String,

    /// Subject metadata (e.g. "Review", "Structure")
    pub subject: String,

    /// Physical tool type
    pub tool_type: ToolType,

    /// Visual style configuration
    pub style: ToolStyle,
}

impl Tool {
    /// Create a new default tool of a specific type
    pub fn new_default(tool_type: ToolType) -> Self {
        let (name, stroke, width, opacity, line_end) = match tool_type {
            ToolType::Pen => (
                "Pen",
                LayerColor::new(255, 0, 0),
                2.0,
                1.0,
                LineEndingType::None,
            ),
            ToolType::Highlighter => (
                "Highlighter",
                LayerColor::new(255, 255, 0),
                12.0,
                0.5,
                LineEndingType::None,
            ),
            ToolType::Line => (
                "Line",
                LayerColor::new(0, 100, 200),
                2.0,
                1.0,
                LineEndingType::None,
            ),
            ToolType::Arrow => (
                "Arrow",
                LayerColor::new(0, 100, 200),
                2.0,
                1.0,
                LineEndingType::Arrow,
            ),
            ToolType::Rectangle => (
                "Rectangle",
                LayerColor::new(0, 0, 255),
                2.0,
                1.0,
                LineEndingType::None,
            ),
            ToolType::Ellipse => (
                "Ellipse",
                LayerColor::new(0, 0, 255),
                2.0,
                1.0,
                LineEndingType::None,
            ),
            ToolType::Cloud => (
                "Cloud",
                LayerColor::new(255, 0, 0),
                2.0,
                1.0,
                LineEndingType::None,
            ),
            ToolType::Text => (
                "Text",
                LayerColor::new(0, 0, 0),
                1.0,
                1.0,
                LineEndingType::None,
            ),
            ToolType::Callout => (
                "Callout",
                LayerColor::new(255, 200, 0),
                2.0,
                1.0,
                LineEndingType::None,
            ),
            ToolType::MeasureLength => (
                "Length",
                LayerColor::new(0, 180, 80),
                2.0,
                1.0,
                LineEndingType::None,
            ),
            ToolType::MeasurePolylength => (
                "Polylength",
                LayerColor::new(0, 180, 80),
                2.0,
                1.0,
                LineEndingType::None,
            ),
            ToolType::MeasureArea => (
                "Area",
                LayerColor::new(0, 120, 200),
                2.0,
                0.8,
                LineEndingType::None,
            ),
            ToolType::Count => (
                "Count",
                LayerColor::new(255, 80, 0),
                2.0,
                1.0,
                LineEndingType::None,
            ),
            ToolType::Viewport => (
                "Viewport",
                LayerColor::new(100, 100, 255),
                2.0,
                0.6,
                LineEndingType::None,
            ),
            ToolType::DimensionChain => (
                "Dim Chain",
                LayerColor::new(0, 180, 80),
                2.0,
                1.0,
                LineEndingType::None,
            ),
        };

        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            subject: name.to_string(),
            tool_type,
            style: ToolStyle {
                stroke_color: stroke,
                fill_color: None,
                opacity,
                line_width: width,
                line_ending_end: line_end,
                ..Default::default()
            },
        }
    }
}
