//! Original CAD geometry, line drawing, and a limited ASCII DXF exchange path.

mod annotation;
mod arc;
mod block;
mod breaking;
mod circle;
mod color;
mod constraints;
mod construction;
mod corner;
mod dxf;
mod editor;
mod ellipse;
mod entity_style;
mod group;
mod joining;
mod layer;
mod layout;
mod lengthen;
mod line;
mod linetype;
mod mesh_ops;
mod modeling;
mod multiline;
mod offset;
mod point;
mod polyline;
mod polyline3;
mod polyline_arc;
mod reference;
mod region;
pub(crate) mod solid;
mod spline;
mod svg;
mod transform;
mod trim;
mod view;

#[cfg(feature = "desktop")]
pub mod desktop;

pub use annotation::{
    AnnotationBounds, Dimension, DimensionKind, DimensionPrimitive, Hatch, HatchPattern,
    HatchPrimitive, HorizontalAlignment, Leader, MLeader, MText, Table, Text, VerticalAlignment,
};
pub use arc::Arc3;
pub use block::{Attribute, AttributeDefinition, BlockDefinition, BlockInsert, Insert};
pub use circle::{Circle3, Entity};
pub use constraints::{
    Anchor, ConstraintError, ConstraintKind, ConstraintRecord, ConstraintSet, LinePair,
};
pub use construction::{Ray3, XLine3};
pub use corner::{Corner, CornerParameter, LineCorner};
pub use dxf::{DxfDocument, DxfError, DxfErrorKind, read_dxf, write_dxf};
pub use editor::{DrawOrder, EditError, Editor};
pub use ellipse::Ellipse3;
pub use entity_style::{EntityStyle, EntityStyleError};
pub use group::EntityGroup;
pub use layer::{Layer, LayerError};
pub use layout::{Layout, LayoutCollection, Orientation, PageSetup, Viewport};
pub use lengthen::Lengthen;
pub use line::{Bounds3, Line3, Point3};
pub use linetype::{Linetype, LinetypeError};
pub use mesh_ops::{
    MeshBooleanOp, MeshOperationError, boundary_edges, mesh_area, mesh_boolean, mesh_volume,
    scale_mesh, subdivide_mesh, transform_mesh, triangulate_mesh,
};
pub use modeling::{
    ModelingError, helix_points, loft_profiles, section_mesh, slice_mesh, sweep_profile,
};
pub use multiline::{MLine2, MLineElement, MLineJustification, MLineStyle};
pub use offset::{Offset, OffsetLayer, OffsetOptions};
pub use point::PointEntity;
pub use polyline::{Polyline2, PolylineSegment, PolylineWidth};
pub use polyline3::Polyline3;
pub use reference::{
    ExternalDxfReference, ExternalReferenceStatus, RasterImage, RasterTriangle, ReferenceManager,
};
pub use region::{Region2, RegionLoop};
pub use solid::{Face3, Mesh3};
pub use spline::Spline3;
pub use svg::{SvgError, render_document_svg, render_entities_svg, render_svg};
pub use transform::Transform;
pub use trim::BoundaryMode;
pub use view::{Camera3, Ucs3, ViewOrientation};

use std::error::Error;
use std::fmt;

/// A point in a Cartesian coordinate system with positive Y upward.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point2 {
    x: f64,
    y: f64,
}

impl Point2 {
    pub fn new(x: f64, y: f64) -> Result<Self, GeometryError> {
        if !x.is_finite() || !y.is_finite() {
            return Err(GeometryError::NonFiniteCoordinate);
        }
        Ok(Self { x, y })
    }

    pub fn x(self) -> f64 {
        self.x
    }

    pub fn y(self) -> f64 {
        self.y
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeometryError {
    NonFiniteCoordinate,
    NonFiniteParameter,
    NonFiniteDistance,
    NonFiniteRadius,
    NegativeRadius,
    NonFiniteAngle,
    InvalidArcSweep,
    InvalidArcChord,
    NonFiniteResult,
    NonPositiveScale,
    NonPositiveOffset,
    UndefinedOffsetSide,
    OffsetCollapse,
    UnrepresentableOffset,
    UnsupportedOffsetEntity,
    UnsupportedTrimEntity,
    UnsupportedExtendEntity,
    UnsupportedBreakEntity,
    UnsupportedBreakAtPoint,
    InvalidBreakPoints,
    UnsupportedPolylineBreak,
    InvalidPolylineBreak,
    UnrepresentableBreak,
    UnsupportedLengthenEntity,
    UnsupportedLengthenAngle,
    InvalidLengthenValue,
    UnrepresentableLengthen,
    UnsupportedJoinEntity,
    UnsupportedJoinClose,
    IncompatibleJoinEntities,
    JoinRequiresFullTurn,
    UnrepresentableJoin,
    UnsupportedPolylineConversion,
    ClosedPolylineJoin,
    DegenerateCurve,
    NoBoundaryIntersection,
    AmbiguousCurvePick,
    InvalidCornerDimension,
    InvalidChamferAngle,
    IncompatibleChamferAngle,
    UnsupportedCornerEntities,
    ParallelCorner,
    AmbiguousCornerPick,
    CornerOutsideSources,
    UnrepresentableCorner,
    DegenerateLine,
    NotEnoughSegmentsToClose,
    NotEnoughVertices,
    NonPlanarPolyline,
    NonFiniteBulge,
    InvalidBulgeCount,
    InvalidWidthCount,
    InvalidPolylineWidth,
    InvalidVertexIndex,
    UnrepresentableBulge,
}

impl fmt::Display for GeometryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFiniteCoordinate => write!(f, "coordinates must be finite"),
            Self::NonFiniteParameter => write!(f, "the parameter must be finite"),
            Self::NonFiniteDistance => write!(f, "the distance must be finite"),
            Self::NonFiniteRadius => write!(f, "the radius must be finite"),
            Self::NegativeRadius => write!(f, "the radius must be nonnegative"),
            Self::NonFiniteAngle => write!(f, "angles must be finite"),
            Self::InvalidArcSweep => {
                write!(
                    f,
                    "an arc's signed sweep must be nonzero and less than one full turn"
                )
            }
            Self::InvalidArcChord => {
                write!(
                    f,
                    "an arc chord must be nonzero and no longer than its diameter"
                )
            }
            Self::NonFiniteResult => write!(f, "the geometry result exceeds the finite range"),
            Self::NonPositiveScale => write!(f, "the scale factor must be positive"),
            Self::NonPositiveOffset => write!(f, "the offset distance must be positive"),
            Self::UndefinedOffsetSide => write!(
                f,
                "choose a point off the source curve to define an offset side"
            ),
            Self::OffsetCollapse => write!(f, "the offset would collapse the curve's radius"),
            Self::UnrepresentableOffset => write!(
                f,
                "the offset does not change geometry at this coordinate precision"
            ),
            Self::UnsupportedOffsetEntity => {
                write!(f, "offset currently supports XY lines, circles and arcs")
            }
            Self::UnsupportedTrimEntity => {
                write!(f, "trim targets must be XY lines, circles or arcs")
            }
            Self::UnsupportedExtendEntity => write!(f, "extend targets must be XY lines or arcs"),
            Self::UnsupportedBreakEntity => {
                write!(f, "break currently supports XY lines, circles and arcs")
            }
            Self::UnsupportedBreakAtPoint => write!(
                f,
                "break at point currently requires an open XY line or arc"
            ),
            Self::InvalidBreakPoints => write!(
                f,
                "choose distinct break points, or one point inside an open curve"
            ),
            Self::UnsupportedPolylineBreak => write!(f, "vertex break requires an XY polyline"),
            Self::InvalidPolylineBreak => write!(
                f,
                "choose at least one interior vertex when breaking an open polyline"
            ),
            Self::UnrepresentableBreak => write!(
                f,
                "the break would produce a fragment below coordinate precision"
            ),
            Self::UnsupportedLengthenEntity => write!(
                f,
                "lengthen currently supports XY lines and arcs; circles are unchanged"
            ),
            Self::UnsupportedLengthenAngle => write!(f, "lengthen Angle requires an arc"),
            Self::InvalidLengthenValue => write!(
                f,
                "lengthen requires a positive final length or an arc sweep between zero and a full turn"
            ),
            Self::UnrepresentableLengthen => {
                write!(f, "the length change is below coordinate precision")
            }
            Self::UnsupportedJoinEntity => write!(f, "join sources must be open XY lines or arcs"),
            Self::UnsupportedJoinClose => write!(f, "Join cLose requires an arc"),
            Self::IncompatibleJoinEntities => write!(
                f,
                "join requires collinear lines or arcs sharing a center, radius and elevation"
            ),
            Self::JoinRequiresFullTurn => write!(
                f,
                "this arc join requires a full turn; use JOIN cLose explicitly"
            ),
            Self::UnrepresentableJoin => write!(f, "the join is below coordinate precision"),
            Self::UnsupportedPolylineConversion => {
                write!(f, "PEDIT requires an XY polyline, line or arc")
            }
            Self::ClosedPolylineJoin => {
                write!(f, "open the source polyline before joining objects")
            }
            Self::DegenerateCurve => write!(f, "the curve has no editable span"),
            Self::NoBoundaryIntersection => {
                write!(f, "no usable boundary intersection for this portion")
            }
            Self::AmbiguousCurvePick => write!(
                f,
                "choose a point inside the portion or nearer one endpoint"
            ),
            Self::InvalidCornerDimension => {
                write!(f, "corner dimensions must be finite and nonnegative")
            }
            Self::InvalidChamferAngle => {
                write!(
                    f,
                    "chamfer angles must be finite, at least 0 and less than 180 degrees"
                )
            }
            Self::IncompatibleChamferAngle => {
                write!(
                    f,
                    "the chamfer angle cannot meet the second line on its picked side"
                )
            }
            Self::UnsupportedCornerEntities => write!(
                f,
                "corners currently require two distinct XY lines at the same elevation"
            ),
            Self::ParallelCorner => write!(
                f,
                "parallel or nearly parallel lines are not supported for corners yet"
            ),
            Self::AmbiguousCornerPick => {
                write!(f, "pick each line on the side of the corner to keep")
            }
            Self::CornerOutsideSources => {
                write!(f, "the corner would leave no line on the picked side")
            }
            Self::UnrepresentableCorner => {
                write!(f, "the corner is too small at this coordinate precision")
            }
            Self::DegenerateLine => write!(f, "a coincident line has no unique distance parameter"),
            Self::NotEnoughSegmentsToClose => write!(f, "closing requires at least two segments"),
            Self::NotEnoughVertices => write!(f, "a polyline requires at least two vertices"),
            Self::NonPlanarPolyline => write!(f, "polyline vertices must share one Z elevation"),
            Self::NonFiniteBulge => write!(f, "bulges must be finite"),
            Self::InvalidBulgeCount => write!(f, "each polyline vertex needs one bulge value"),
            Self::InvalidWidthCount => write!(f, "each polyline vertex needs one width pair"),
            Self::InvalidPolylineWidth => {
                write!(f, "polyline widths must be finite and nonnegative")
            }
            Self::InvalidVertexIndex => write!(f, "the polyline vertex index is out of range"),
            Self::UnrepresentableBulge => {
                write!(f, "the bulged segment exceeds representable geometry")
            }
        }
    }
}

impl Error for GeometryError {}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineSegment {
    start: Point2,
    end: Point2,
}

impl LineSegment {
    pub fn start(self) -> Point2 {
        self.start
    }

    pub fn end(self) -> Point2 {
        self.end
    }
}

/// A drawing of independent line segments. Successive clicks form a chain.
#[derive(Debug, Default)]
pub struct Drawing {
    segments: Vec<LineSegment>,
    chain_start: Option<Point2>,
    chain_end: Option<Point2>,
    chain_segment_start: usize,
}

impl Drawing {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start or extend the current chain. Returns the newly created segment, if any.
    pub fn click(&mut self, point: Point2) -> Option<LineSegment> {
        if self.chain_end.is_none() {
            self.chain_start = Some(point);
            self.chain_segment_start = self.segments.len();
        }
        let segment = self
            .chain_end
            .map(|start| LineSegment { start, end: point });
        if let Some(segment) = segment {
            self.segments.push(segment);
        }
        self.chain_end = Some(point);
        segment
    }

    /// Remove the most recent segment of the active chain, if one exists.
    pub fn undo_last_segment(&mut self) -> Option<LineSegment> {
        if self.segments.len() == self.chain_segment_start {
            return None;
        }
        let segment = self.segments.pop()?;
        self.chain_end = Some(segment.start());
        Some(segment)
    }

    /// Connect the active chain's last point to its first, then end the chain.
    pub fn close_chain(&mut self) -> Result<LineSegment, GeometryError> {
        if self.segments.len() - self.chain_segment_start < 2 {
            return Err(GeometryError::NotEnoughSegmentsToClose);
        }
        let segment = LineSegment {
            start: self
                .chain_end
                .expect("a chain with segments has an endpoint"),
            end: self.chain_start.expect("a chain with segments has a start"),
        };
        self.segments.push(segment);
        self.finish_chain();
        Ok(segment)
    }

    /// End the current chain; the next click starts a new one.
    pub fn finish_chain(&mut self) {
        self.chain_start = None;
        self.chain_end = None;
        self.chain_segment_start = self.segments.len();
    }

    pub fn segments(&self) -> &[LineSegment] {
        &self.segments
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_reject_non_finite_coordinates() {
        assert_eq!(
            Point2::new(f64::NAN, 0.0),
            Err(GeometryError::NonFiniteCoordinate)
        );
        assert_eq!(
            Point2::new(0.0, f64::INFINITY),
            Err(GeometryError::NonFiniteCoordinate)
        );
    }

    #[test]
    fn clicks_make_connected_segments_and_finish_starts_a_new_chain() {
        let a = Point2::new(0.0, 0.0).unwrap();
        let b = Point2::new(10.0, 0.0).unwrap();
        let c = Point2::new(10.0, 5.0).unwrap();
        let d = Point2::new(20.0, 20.0).unwrap();
        let mut drawing = Drawing::new();

        assert_eq!(drawing.click(a), None);
        assert_eq!(drawing.click(b).unwrap().start(), a);
        assert_eq!(drawing.click(c).unwrap().start(), b);
        drawing.finish_chain();
        assert_eq!(drawing.click(d), None);
        assert_eq!(drawing.segments().len(), 2);
        assert_eq!(drawing.segments()[1].end(), c);
    }

    #[test]
    fn undo_and_close_change_only_the_active_line_sequence() {
        let a = Point2::new(0.0, 0.0).unwrap();
        let b = Point2::new(10.0, 0.0).unwrap();
        let c = Point2::new(10.0, 10.0).unwrap();
        let d = Point2::new(20.0, 20.0).unwrap();
        let e = Point2::new(30.0, 20.0).unwrap();
        let mut drawing = Drawing::new();

        drawing.click(a);
        drawing.click(b);
        drawing.finish_chain();
        drawing.click(b);
        drawing.click(c);
        drawing.click(d);
        drawing.click(e);
        assert_eq!(drawing.undo_last_segment().unwrap().end(), e);
        assert_eq!(drawing.close_chain().unwrap().end(), b);

        assert_eq!(drawing.segments().len(), 4);
        assert_eq!(drawing.segments()[0].start(), a);
        assert_eq!(drawing.segments()[1].end(), c);
        assert_eq!(drawing.segments()[2].start(), c);
        assert_eq!(drawing.segments()[3].start(), d);
        assert_eq!(drawing.undo_last_segment(), None);
    }

    #[test]
    fn close_requires_two_active_segments() {
        let mut drawing = Drawing::new();
        drawing.click(Point2::new(0.0, 0.0).unwrap());
        drawing.click(Point2::new(10.0, 0.0).unwrap());
        assert_eq!(
            drawing.close_chain(),
            Err(GeometryError::NotEnoughSegmentsToClose)
        );
        assert_eq!(drawing.segments().len(), 1);
    }
}
