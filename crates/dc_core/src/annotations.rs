// =============================================================================
// dc_core/annotations - Vector Annotation Primitives
// =============================================================================
// Defines the data structures for vector annotations (paths, shapes, text).
// =============================================================================

use crate::tools::ToolStyle;
use crate::types::LayerId;
use serde::{Deserialize, Serialize};

/// A point in 2D space (image coordinates).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    /// X coordinate in pixels
    pub x: f32,
    /// Y coordinate in pixels
    pub y: f32,
}

impl Point {
    /// Create a new point from coordinates
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// A specific annotation instance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Annotation {
    /// Unique identifier
    pub id: String,

    /// Layer this annotation belongs to
    pub layer_id: LayerId,

    /// Visual style
    pub style: ToolStyle,

    /// The geometric data
    pub data: AnnotationData,

    /// Bounding box (min_x, min_y, max_x, max_y)
    pub bounds: (f32, f32, f32, f32),

    /// Custom user-defined properties (key-value pairs)
    #[serde(default)]
    pub custom_properties: std::collections::HashMap<String, String>,
}

/// The geometric data for an annotation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AnnotationData {
    /// Freehand path or polyline
    Path(Vec<Point>),

    /// Axis-aligned rectangle defined by two corners
    Rectangle {
        /// Top-left corner
        start: Point,
        /// Bottom-right corner
        end: Point,
    },

    /// Ellipse defined by bounding box
    Ellipse {
        /// Top-left of bounding box
        start: Point,
        /// Bottom-right of bounding box
        end: Point,
    },

    /// Revision cloud (sequence of points defining the hull)
    Cloud(Vec<Point>),

    /// Line segment with optional arrowheads (handled by style/type implied)
    Line {
        /// Start point
        start: Point,
        /// End point
        end: Point,
    },

    /// Text content at a location
    Text {
        /// Position of the text anchor
        pos: Point,
        /// The text content string
        content: String,
        /// Optional wrapping width in pixels
        width: Option<f32>, // Optional wrapping width
    },

    /// A measurement annotation (length, polylength, or area)
    Measurement {
        /// The measured geometry (polyline points — two points = length, many = polylength/area)
        points: Vec<Point>,
        /// Whether this is an area measurement (polygon is closed)
        is_area: bool,
        /// Computed value in calibrated units (or pixels if uncalibrated)
        value: f64,
        /// Label string (e.g. "12.34 mm")
        label: String,
    },

    /// A count marker at a location
    Count {
        /// Center position of the marker
        pos: Point,
        /// The count number displayed
        number: u32,
        /// Optional sequence group for punch list grouping.
        /// Markers with the same group_id form a numbered sequence.
        #[serde(default)]
        sequence_group_id: Option<String>,
    },

    /// A viewport rectangle defining a local scale override.
    /// All measurements drawn inside this rectangle use `scale` instead of
    /// the document global `pixels_per_unit`.
    Viewport {
        /// Top-left corner
        start: Point,
        /// Bottom-right corner
        end: Point,
        /// Local scale: pixels per unit inside this viewport
        scale: f64,
        /// Scale label (e.g. "1:10", "1:50")
        label: String,
    },

    /// A continuous dimension chain (A──B──C renders tick marks + segment labels).
    DimensionChain {
        /// Ordered list of chain nodes
        points: Vec<Point>,
        /// Computed segment lengths in calibrated units
        segment_values: Vec<f64>,
        /// Segment label strings (e.g. "12.34 mm")
        segment_labels: Vec<String>,
        /// Total chain length (calibrated)
        total_value: f64,
        /// Total label string
        total_label: String,
    },
}

impl Annotation {
    /// Create a new annotation
    pub fn new(layer_id: LayerId, style: ToolStyle, data: AnnotationData) -> Self {
        let bounds = data.compute_bounds();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            layer_id,
            style,
            data,
            bounds,
            custom_properties: std::collections::HashMap::new(),
        }
    }

    /// Transform the annotation using a homography matrix.
    pub fn transform_by(&mut self, homography: &crate::alignment::HomographyMatrix) {
        self.data.transform_by(homography);
        self.bounds = self.data.compute_bounds();
    }
}

impl AnnotationData {
    /// Transform the geometry using a homography matrix
    pub fn transform_by(&mut self, h: &crate::alignment::HomographyMatrix) {
        let tp = |p: Point| -> Point {
            let (x, y) = h.transform_point(p.x as f64, p.y as f64);
            Point {
                x: x as f32,
                y: y as f32,
            }
        };

        match self {
            Self::Path(points) | Self::Cloud(points) => {
                for p in points.iter_mut() {
                    *p = tp(*p);
                }
            }
            Self::Rectangle { start, end }
            | Self::Ellipse { start, end }
            | Self::Line { start, end } => {
                *start = tp(*start);
                *end = tp(*end);
            }
            Self::Text { pos, .. } | Self::Count { pos, .. } => {
                *pos = tp(*pos);
            }
            Self::Measurement { points, .. } => {
                for p in points.iter_mut() {
                    *p = tp(*p);
                }
                // We should recompute area/length, but for now we'll just move the points.
                // The recompute needs correct scale. In slip-sheeting the scale usually stays similar.
            }
            Self::Viewport { start, end, .. } => {
                *start = tp(*start);
                *end = tp(*end);
            }
            Self::DimensionChain { points, .. } => {
                for p in points.iter_mut() {
                    *p = tp(*p);
                }
            }
        }
    }

    /// Compute the bounding box of the geometry
    pub fn compute_bounds(&self) -> (f32, f32, f32, f32) {
        match self {
            Self::Path(points) | Self::Cloud(points) => {
                if points.is_empty() {
                    return (0.0, 0.0, 0.0, 0.0);
                }
                let mut min_x = f32::MAX;
                let mut min_y = f32::MAX;
                let mut max_x = f32::MIN;
                let mut max_y = f32::MIN;

                for p in points {
                    min_x = min_x.min(p.x);
                    min_y = min_y.min(p.y);
                    max_x = max_x.max(p.x);
                    max_y = max_y.max(p.y);
                }
                (min_x, min_y, max_x, max_y)
            }
            Self::Rectangle { start, end }
            | Self::Ellipse { start, end }
            | Self::Line { start, end } => (
                start.x.min(end.x),
                start.y.min(end.y),
                start.x.max(end.x),
                start.y.max(end.y),
            ),
            Self::Text { pos, width, .. } => {
                // Text bounds are approximate without font metrics.
                // We'll just define it as a small box at the position for now.
                let w = width.unwrap_or(100.0);
                (pos.x, pos.y, pos.x + w, pos.y + 20.0)
            }
            Self::Measurement { points, .. } => {
                if points.is_empty() {
                    return (0.0, 0.0, 0.0, 0.0);
                }
                let mut min_x = f32::MAX;
                let mut min_y = f32::MAX;
                let mut max_x = f32::MIN;
                let mut max_y = f32::MIN;
                for p in points {
                    min_x = min_x.min(p.x);
                    min_y = min_y.min(p.y);
                    max_x = max_x.max(p.x);
                    max_y = max_y.max(p.y);
                }
                (min_x, min_y, max_x, max_y)
            }
            Self::Count { pos, .. } => (pos.x - 10.0, pos.y - 10.0, pos.x + 10.0, pos.y + 10.0),
            Self::Viewport { start, end, .. } => (
                start.x.min(end.x),
                start.y.min(end.y),
                start.x.max(end.x),
                start.y.max(end.y),
            ),
            Self::DimensionChain { points, .. } => {
                if points.is_empty() {
                    return (0.0, 0.0, 0.0, 0.0);
                }
                let mut min_x = f32::MAX;
                let mut min_y = f32::MAX;
                let mut max_x = f32::MIN;
                let mut max_y = f32::MIN;
                for p in points {
                    min_x = min_x.min(p.x);
                    min_y = min_y.min(p.y);
                    max_x = max_x.max(p.x);
                    max_y = max_y.max(p.y);
                }
                (min_x, min_y, max_x, max_y)
            }
        }
    }

    /// Check if a point contains the given coordinate
    pub fn contains(&self, p: Point, tolerance: f32) -> bool {
        match self {
            Self::Rectangle { start, end } | Self::Ellipse { start, end } => {
                let min_x = start.x.min(end.x) - tolerance;
                let max_x = start.x.max(end.x) + tolerance;
                let min_y = start.y.min(end.y) - tolerance;
                let max_y = start.y.max(end.y) + tolerance;

                p.x >= min_x && p.x <= max_x && p.y >= min_y && p.y <= max_y
            }
            Self::Path(points) | Self::Cloud(points) => {
                // Simple bounding box check first
                let (min_x, min_y, max_x, max_y) = self.compute_bounds();
                if p.x < min_x - tolerance
                    || p.x > max_x + tolerance
                    || p.y < min_y - tolerance
                    || p.y > max_y + tolerance
                {
                    return false;
                }

                // Check distance to any segment (simple)
                // For filled shapes we'd need point-in-polygon
                if points.len() < 2 {
                    return false;
                }
                for w in points.windows(2) {
                    let p1 = w[0];
                    let p2 = w[1];
                    if distance_to_segment(p, p1, p2) <= tolerance {
                        return true;
                    }
                }
                false
            }
            Self::Line { start, end } => distance_to_segment(p, *start, *end) <= tolerance,
            Self::Text { pos, width, .. } => {
                let w = width.unwrap_or(100.0);
                p.x >= pos.x && p.x <= pos.x + w && p.y >= pos.y && p.y <= pos.y + 20.0
            }
            Self::Measurement {
                points, is_area, ..
            } => {
                if points.len() < 2 {
                    return false;
                }
                let (min_x, min_y, max_x, max_y) = self.compute_bounds();
                if p.x < min_x - tolerance
                    || p.x > max_x + tolerance
                    || p.y < min_y - tolerance
                    || p.y > max_y + tolerance
                {
                    return false;
                }
                for w in points.windows(2) {
                    if distance_to_segment(p, w[0], w[1]) <= tolerance {
                        return true;
                    }
                }
                // For area, also check closing segment
                if *is_area && points.len() > 2 {
                    if distance_to_segment(p, *points.last().unwrap(), points[0]) <= tolerance {
                        return true;
                    }
                }
                false
            }
            Self::Count { pos, .. } => {
                let dx = p.x - pos.x;
                let dy = p.y - pos.y;
                (dx * dx + dy * dy).sqrt() <= tolerance + 12.0
            }
            Self::Viewport { start, end, .. } => {
                // Hit test on the rectangle edges (not interior) so we can still
                // interact with annotations inside the viewport.
                let min_x = start.x.min(end.x);
                let max_x = start.x.max(end.x);
                let min_y = start.y.min(end.y);
                let max_y = start.y.max(end.y);

                // Check if near any edge
                let near_left = (p.x - min_x).abs() <= tolerance
                    && p.y >= min_y - tolerance
                    && p.y <= max_y + tolerance;
                let near_right = (p.x - max_x).abs() <= tolerance
                    && p.y >= min_y - tolerance
                    && p.y <= max_y + tolerance;
                let near_top = (p.y - min_y).abs() <= tolerance
                    && p.x >= min_x - tolerance
                    && p.x <= max_x + tolerance;
                let near_bottom = (p.y - max_y).abs() <= tolerance
                    && p.x >= min_x - tolerance
                    && p.x <= max_x + tolerance;

                near_left || near_right || near_top || near_bottom
            }
            Self::DimensionChain { points, .. } => {
                if points.len() < 2 {
                    return false;
                }
                let (min_x, min_y, max_x, max_y) = self.compute_bounds();
                if p.x < min_x - tolerance
                    || p.x > max_x + tolerance
                    || p.y < min_y - tolerance
                    || p.y > max_y + tolerance
                {
                    return false;
                }
                for w in points.windows(2) {
                    if distance_to_segment(p, w[0], w[1]) <= tolerance {
                        return true;
                    }
                }
                false
            }
        }
    }
}

fn distance_to_segment(p: Point, start: Point, end: Point) -> f32 {
    let l2 = (start.x - end.x).powi(2) + (start.y - end.y).powi(2);
    if l2 == 0.0 {
        return ((p.x - start.x).powi(2) + (p.y - start.y).powi(2)).sqrt();
    }

    let t = ((p.x - start.x) * (end.x - start.x) + (p.y - start.y) * (end.y - start.y)) / l2;
    let t_clamped = t.max(0.0).min(1.0);

    let proj_x = start.x + t_clamped * (end.x - start.x);
    let proj_y = start.y + t_clamped * (end.y - start.y);

    ((p.x - proj_x).powi(2) + (p.y - proj_y).powi(2)).sqrt()
}

// =============================================================================
// Calibration
// =============================================================================

/// Calibration: maps image-pixel distances to real-world units.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Calibration {
    /// Pixels per unit (e.g. pixels per millimetre).
    pub pixels_per_unit: f64,
    /// Unit name (e.g. "mm", "in", "ft").
    pub unit: String,
}

impl Default for Calibration {
    fn default() -> Self {
        Self {
            pixels_per_unit: 1.0,
            unit: "px".to_string(),
        }
    }
}

impl Calibration {
    /// Convert a pixel distance to calibrated units.
    pub fn to_units(&self, pixels: f64) -> f64 {
        pixels / self.pixels_per_unit
    }

    /// Convert a pixel distance using a specific scale override.
    pub fn to_units_with_scale(&self, pixels: f64, scale: f64) -> f64 {
        pixels / scale
    }

    /// Format a distance value with the unit label.
    pub fn format_length(&self, pixels: f64) -> String {
        let val = self.to_units(pixels);
        format!("{:.2} {}", val, self.unit)
    }

    /// Format a distance value using a specific scale override.
    pub fn format_length_with_scale(&self, pixels: f64, scale: f64) -> String {
        let val = self.to_units_with_scale(pixels, scale);
        format!("{:.2} {}", val, self.unit)
    }

    /// Format an area value (units²).
    pub fn format_area(&self, px_area: f64) -> String {
        let unit_area = px_area / (self.pixels_per_unit * self.pixels_per_unit);
        format!("{:.2} {}²", unit_area, self.unit)
    }

    /// Format an area value using a specific scale override.
    pub fn format_area_with_scale(&self, px_area: f64, scale: f64) -> String {
        let unit_area = px_area / (scale * scale);
        format!("{:.2} {}²", unit_area, self.unit)
    }
}

// =============================================================================
// Measurement Helpers
// =============================================================================

/// Compute the Euclidean distance between two points (in pixels).
pub fn point_distance(a: Point, b: Point) -> f64 {
    let dx = (b.x - a.x) as f64;
    let dy = (b.y - a.y) as f64;
    (dx * dx + dy * dy).sqrt()
}

/// Compute cumulative length of a polyline (in pixels).
pub fn polyline_length(points: &[Point]) -> f64 {
    if points.len() < 2 {
        return 0.0;
    }
    points.windows(2).map(|w| point_distance(w[0], w[1])).sum()
}

/// Compute the area of a polygon using the Shoelace formula (in pixels²).
pub fn polygon_area(points: &[Point]) -> f64 {
    if points.len() < 3 {
        return 0.0;
    }
    let mut sum = 0.0_f64;
    let n = points.len();
    for i in 0..n {
        let j = (i + 1) % n;
        sum += (points[i].x as f64) * (points[j].y as f64);
        sum -= (points[j].x as f64) * (points[i].y as f64);
    }
    (sum / 2.0).abs()
}

/// Recompute the segment values and labels for a dimension chain.
pub fn compute_dimension_chain(
    points: &[Point],
    calibration: &Calibration,
    viewport_scale: Option<f64>,
) -> (Vec<f64>, Vec<String>, f64, String) {
    let scale = viewport_scale.unwrap_or(calibration.pixels_per_unit);
    let mut segment_values = Vec::new();
    let mut segment_labels = Vec::new();
    let mut total = 0.0_f64;

    for pair in points.windows(2) {
        let px_dist = point_distance(pair[0], pair[1]);
        let unit_val = px_dist / scale;
        segment_values.push(unit_val);
        segment_labels.push(format!("{:.2} {}", unit_val, calibration.unit));
        total += unit_val;
    }

    let total_label = format!("Σ {:.2} {}", total, calibration.unit);
    (segment_values, segment_labels, total, total_label)
}

/// Get the next sequence number for a given punch list group.
/// Scans all layers for Count annotations matching the group_id and returns max+1.
pub fn next_sequence_number(layers: &[crate::Layer], group_id: &str) -> u32 {
    let mut max_num = 0u32;
    for layer in layers {
        for annot in &layer.annotations {
            if let AnnotationData::Count {
                number,
                sequence_group_id: Some(ref gid),
                ..
            } = annot.data
            {
                if gid == group_id {
                    max_num = max_num.max(number);
                }
            }
        }
    }
    max_num + 1
}
