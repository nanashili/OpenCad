use crate::constraints::{Anchor, ConstraintError, ConstraintKind, ConstraintRecord};
use crate::{
    BlockDefinition, BoundaryMode, Corner, CornerParameter, DxfDocument, Entity, EntityGroup,
    EntitySpace, EntityStyle, EntityStyleError, GeometryError, Layer, LayerError, LayoutCollection,
    Lengthen, Linetype, LinetypeError, Offset, OffsetLayer, OffsetOptions, Point3, Transform,
};
use std::{error::Error, fmt};

type LayerState = (String, bool, bool, bool, bool);

#[derive(Debug)]
pub enum EditError {
    Geometry(GeometryError),
    BlockExpansion(crate::BlockExpansionError),
    Constraint(ConstraintError),
    Layer(LayerError),
    Style(EntityStyleError),
    Dxf(crate::DxfError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawOrder {
    Front,
    Back,
    Above(usize),
    Below(usize),
}
impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Geometry(error) => error.fmt(f),
            Self::BlockExpansion(error) => error.fmt(f),
            Self::Constraint(error) => error.fmt(f),
            Self::Layer(error) => error.fmt(f),
            Self::Style(error) => error.fmt(f),
            Self::Dxf(error) => error.fmt(f),
        }
    }
}
impl Error for EditError {}
impl From<GeometryError> for EditError {
    fn from(error: GeometryError) -> Self {
        Self::Geometry(error)
    }
}
impl From<crate::BlockExpansionError> for EditError {
    fn from(error: crate::BlockExpansionError) -> Self {
        Self::BlockExpansion(error)
    }
}
impl From<ConstraintError> for EditError {
    fn from(error: ConstraintError) -> Self {
        Self::Constraint(error)
    }
}
impl From<LayerError> for EditError {
    fn from(error: LayerError) -> Self {
        Self::Layer(error)
    }
}
impl From<EntityStyleError> for EditError {
    fn from(error: EntityStyleError) -> Self {
        Self::Style(error)
    }
}
impl From<crate::DxfError> for EditError {
    fn from(error: crate::DxfError) -> Self {
        Self::Dxf(error)
    }
}

#[derive(Clone)]
struct Snapshot {
    document: DxfDocument,
    layouts: LayoutCollection,
    selection: Vec<usize>,
    revision: u64,
}

/// Editable drawing state. Each user action is one undo transaction, including
/// actions involving several entities. Imported document records survive undo.
pub struct Editor {
    current: Snapshot,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    next_revision: u64,
    saved_revision: u64,
    active_layer: String,
    layer_visibility_restore: Option<Vec<LayerState>>,
    layer_previous_state: Option<Vec<LayerState>>,
}

impl Default for Editor {
    fn default() -> Self {
        Self::new(DxfDocument::default())
    }
}

impl Editor {
    /// Build a new-position-to-old-position permutation for draw-order edits.
    pub fn draw_order_permutation(
        entity_count: usize,
        selected: &[usize],
        order: DrawOrder,
    ) -> Result<Vec<usize>, EditError> {
        let mut selected = selected.to_vec();
        selected.sort_unstable();
        selected.dedup();
        if selected.iter().any(|index| *index >= entity_count) {
            return Err(LayerError::MissingEntity(entity_count).into());
        }
        let is_selected = |index: usize| selected.binary_search(&index).is_ok();
        let mut remaining = (0..entity_count)
            .filter(|index| !is_selected(*index))
            .collect::<Vec<_>>();
        match order {
            DrawOrder::Front => remaining.extend(selected),
            DrawOrder::Back => {
                selected.extend(remaining);
                remaining = selected;
            }
            DrawOrder::Above(reference) | DrawOrder::Below(reference) => {
                if reference >= entity_count || is_selected(reference) {
                    return Err(LayerError::MissingEntity(reference).into());
                }
                let Some(position) = remaining.iter().position(|index| *index == reference) else {
                    return Err(LayerError::MissingEntity(reference).into());
                };
                let insert_at = if matches!(order, DrawOrder::Above(_)) {
                    position + 1
                } else {
                    position
                };
                remaining.splice(insert_at..insert_at, selected);
            }
        }
        Ok(remaining)
    }

    /// Apply a validated entity-order permutation through a document callback
    /// while retaining source metadata and remapping the selection in one undo
    /// transaction. The callback is intended for DxfDocument::reorder_entities.
    pub fn reorder_entities_with(
        &mut self,
        order: &[usize],
        reorder: impl FnOnce(&mut DxfDocument, &[usize]) -> Result<bool, crate::DxfError>,
    ) -> Result<bool, EditError> {
        self.editable_selection()?;
        if order.len() != self.entities().len() {
            return Err(crate::DxfError {
                line: 0,
                kind: crate::DxfErrorKind::UnsupportedProperty(0),
            }
            .into());
        }
        let mut seen = vec![false; order.len()];
        for &index in order {
            if index >= seen.len() || seen[index] {
                return Err(crate::DxfError {
                    line: 0,
                    kind: crate::DxfErrorKind::UnsupportedProperty(0),
                }
                .into());
            }
            seen[index] = true;
        }
        let mut candidate = self.current.document.clone();
        if !reorder(&mut candidate, order)? {
            return Ok(false);
        }
        let mut inverse = vec![0; order.len()];
        for (new_index, old_index) in order.iter().copied().enumerate() {
            inverse[old_index] = new_index;
        }
        let selection = self
            .selection()
            .iter()
            .filter_map(|index| inverse.get(*index).copied())
            .collect();
        self.checkpoint();
        self.install_document(candidate);
        self.current.selection = selection;
        Ok(true)
    }

    /// Reorder entities while retaining all parallel document metadata.
    pub fn reorder_entities(&mut self, order: &[usize]) -> Result<bool, EditError> {
        self.reorder_entities_with(order, |document, order| document.reorder_entities(order))
    }

    pub fn new(document: DxfDocument) -> Self {
        let layouts = document.layouts().clone();
        Self {
            current: Snapshot {
                document,
                layouts,
                selection: Vec::new(),
                revision: 0,
            },
            undo: Vec::new(),
            redo: Vec::new(),
            next_revision: 1,
            saved_revision: 0,
            active_layer: "0".into(),
            layer_visibility_restore: None,
            layer_previous_state: None,
        }
    }

    pub fn entities(&self) -> &[Entity] {
        self.current.document.entities()
    }
    pub fn groups(&self) -> &[EntityGroup] {
        self.current.document.groups()
    }
    pub fn selection(&self) -> &[usize] {
        &self.current.selection
    }
    pub fn layouts(&self) -> &LayoutCollection {
        &self.current.layouts
    }
    pub fn active_space(&self) -> EntitySpace {
        let name = self.current.layouts.active().name();
        if name.eq_ignore_ascii_case("Model") {
            EntitySpace::Model
        } else {
            EntitySpace::Paper(name.to_owned())
        }
    }
    pub fn entity_space(&self, index: usize) -> Option<&EntitySpace> {
        self.current.document.entity_space(index)
    }
    fn canonical_layout_name(&self, name: &str) -> Option<String> {
        self.current
            .layouts
            .names()
            .find(|candidate| candidate.eq_ignore_ascii_case(name))
            .map(str::to_owned)
    }
    fn canonical_entity_space(&self, space: EntitySpace) -> Result<EntitySpace, EditError> {
        match space {
            EntitySpace::Model => Ok(EntitySpace::Model),
            EntitySpace::Paper(layout) => {
                let canonical_layout = if layout.eq_ignore_ascii_case("Model") {
                    None
                } else {
                    self.canonical_layout_name(&layout)
                };
                canonical_layout.map(EntitySpace::Paper).ok_or_else(|| {
                    crate::DxfError {
                        line: 0,
                        kind: crate::DxfErrorKind::UnsupportedProperty(410),
                    }
                    .into()
                })
            }
        }
    }
    pub fn constraints(&self) -> &[ConstraintRecord] {
        self.current.document.constraints()
    }

    pub fn add_constraint(
        &mut self,
        kind: ConstraintKind,
        value: Option<f64>,
    ) -> Result<u64, ConstraintError> {
        let mut candidate = self.current.document.clone();
        let id = candidate.add_constraint(kind, value)?;
        self.checkpoint();
        self.install_document(candidate);
        Ok(id)
    }

    pub fn add_fixed_constraint(
        &mut self,
        anchor: Anchor,
        point: Point3,
    ) -> Result<u64, ConstraintError> {
        let mut candidate = self.current.document.clone();
        let id = candidate.add_fixed_constraint(anchor, point)?;
        self.checkpoint();
        self.install_document(candidate);
        Ok(id)
    }

    /// Pin a supported entity's complete 2D definition in one metadata edit.
    /// Lines pin both endpoints; circles pin center and radius; points pin
    /// their position. The document solver then preserves the definition on
    /// subsequent geometry edits.
    pub fn fix_entity_constraint(&mut self, index: usize) -> Result<Vec<u64>, ConstraintError> {
        let entity = self
            .entities()
            .get(index)
            .ok_or(ConstraintError::InvalidReference(index))?;
        let mut candidate = self.current.document.clone();
        let mut ids = Vec::new();
        match entity {
            Entity::Point(point) => ids.push(
                candidate.add_fixed_constraint(Anchor::EntityPoint(index), point.position())?,
            ),
            Entity::Line(line) => {
                ids.push(candidate.add_fixed_constraint(Anchor::LineStart(index), line.start())?);
                ids.push(candidate.add_fixed_constraint(Anchor::LineEnd(index), line.end())?);
            }
            Entity::Circle(circle) => {
                ids.push(
                    candidate.add_fixed_constraint(Anchor::CircleCenter(index), circle.center())?,
                );
                ids.push(
                    candidate
                        .add_constraint(ConstraintKind::Radius(index), Some(circle.radius()))?,
                );
            }
            _ => {
                return Err(ConstraintError::UnsupportedGeometry(
                    "a point, line, or circle",
                ));
            }
        }
        self.checkpoint();
        self.install_document(candidate);
        Ok(ids)
    }

    pub fn remove_constraint(&mut self, id: u64) -> Result<ConstraintRecord, ConstraintError> {
        let mut candidate = self.current.document.clone();
        let removed = candidate.remove_constraint(id)?;
        self.checkpoint();
        self.install_document(candidate);
        Ok(removed)
    }

    pub fn set_constraint_enabled(
        &mut self,
        id: u64,
        enabled: bool,
    ) -> Result<bool, ConstraintError> {
        let mut candidate = self.current.document.clone();
        if !candidate.set_constraint_enabled(id, enabled)? {
            return Ok(false);
        }
        self.commit_constraint_candidate(candidate)
    }

    pub fn set_constraint_value(&mut self, id: u64, value: f64) -> Result<bool, ConstraintError> {
        let mut candidate = self.current.document.clone();
        if !candidate.set_constraint_value(id, value)? {
            return Ok(false);
        }
        self.commit_constraint_candidate(candidate)
    }

    fn commit_constraint_candidate(
        &mut self,
        mut candidate: DxfDocument,
    ) -> Result<bool, ConstraintError> {
        candidate.solve_constraints()?;
        let geometry_changed = self.entities() != candidate.entities();
        let metadata_changed = self.constraints() != candidate.constraints();
        if !geometry_changed && !metadata_changed {
            return Ok(false);
        }
        for (index, (before, after)) in self.entities().iter().zip(candidate.entities()).enumerate()
        {
            if before != after {
                self.editable_layer(self.entity_layer(index).ok_or(
                    ConstraintError::Inconsistent("constrained entity no longer exists"),
                )?)
                .map_err(|_| {
                    ConstraintError::Inconsistent(
                        "constrained entity is on a locked or hidden layer",
                    )
                })?;
            }
        }
        self.checkpoint();
        self.install_document(candidate);
        Ok(true)
    }

    /// Solve all enabled constraints against a cloned document and commit only
    /// when every constrained edit is valid and its layer is editable.
    pub fn solve_constraints(&mut self) -> Result<bool, ConstraintError> {
        let mut candidate = self.current.document.clone();
        if !candidate.solve_constraints()? {
            return Ok(false);
        }
        for (index, (before, after)) in self.entities().iter().zip(candidate.entities()).enumerate()
        {
            if before != after {
                self.editable_layer(self.entity_layer(index).ok_or(
                    ConstraintError::Inconsistent("constrained entity no longer exists"),
                )?)
                .map_err(|_| {
                    ConstraintError::Inconsistent(
                        "constrained entity is on a locked or hidden layer",
                    )
                })?;
            }
        }
        self.checkpoint();
        self.install_document(candidate);
        Ok(true)
    }
    pub fn layers(&self) -> &[Layer] {
        self.current.document.layers()
    }
    #[cfg(feature = "desktop")]
    pub(crate) fn document(&self) -> &DxfDocument {
        &self.current.document
    }
    #[cfg(feature = "desktop")]
    pub(crate) fn revision(&self) -> u64 {
        self.current.revision
    }
    pub fn linetypes(&self) -> &[Linetype] {
        self.current.document.linetypes()
    }
    pub fn linetype_scale(&self) -> f64 {
        self.current.document.linetype_scale()
    }
    pub fn current_layer(&self) -> &str {
        &self.active_layer
    }
    /// The current creation layer is workspace state; choosing it is not a
    /// drawing edit. Entity membership and layer properties are persisted.
    pub fn set_current_layer(&mut self, name: &str) -> Result<(), LayerError> {
        self.active_layer = self
            .current
            .document
            .layer(name)
            .ok_or_else(|| LayerError::UnknownLayer(name.into()))?
            .name()
            .into();
        Ok(())
    }

    /// Make the first selected entity's layer the current creation layer.
    pub fn make_selection_layer_current(&mut self) -> Result<bool, LayerError> {
        let Some(&index) = self.selection().first() else {
            return Ok(false);
        };
        let layer = self
            .entity_layer(index)
            .ok_or(LayerError::MissingEntity(index))?
            .to_owned();
        self.set_current_layer(&layer)?;
        Ok(true)
    }
    pub fn entity_layer(&self, index: usize) -> Option<&str> {
        self.current.document.entity_layer(index)
    }
    pub fn layer_for_entity(&self, index: usize) -> Option<&Layer> {
        self.current.document.layer_for_entity(index)
    }
    pub fn entity_style(&self, index: usize) -> Option<&EntityStyle> {
        self.current.document.entity_style(index)
    }
    pub fn is_entity_visible(&self, index: usize) -> bool {
        let active_layout = self.current.layouts.active().name();
        self.layer_for_entity(index).is_some_and(Layer::is_visible)
            && self
                .entity_space(index)
                .is_some_and(|space| space.matches_layout(active_layout))
    }
    pub fn visible_entities(&self) -> Vec<Entity> {
        self.entities()
            .iter()
            .enumerate()
            .filter(|(index, _)| self.is_entity_visible(*index))
            .map(|(_, entity)| entity.clone())
            .collect()
    }

    fn editable_layer(&self, name: &str) -> Result<(), LayerError> {
        let layer = self
            .current
            .document
            .layer(name)
            .ok_or_else(|| LayerError::UnknownLayer(name.into()))?;
        if layer.is_locked() {
            return Err(LayerError::LockedLayer(layer.name().into()));
        }
        if !layer.is_visible() {
            return Err(LayerError::HiddenLayer(layer.name().into()));
        }
        Ok(())
    }
    fn editable_selection(&self) -> Result<(), LayerError> {
        for index in self.selection() {
            self.editable_layer(self.entity_layer(*index).expect("valid selection"))?;
        }
        Ok(())
    }

    fn edit_document<E>(
        &mut self,
        edit: impl FnOnce(&mut DxfDocument) -> Result<bool, E>,
    ) -> Result<bool, E> {
        let mut document = self.current.document.clone();
        if !edit(&mut document)? {
            return Ok(false);
        }
        self.checkpoint();
        self.install_document(document);
        self.select(self.selection().to_vec());
        Ok(true)
    }

    /// Replace the drawing document after an external workflow has modified a
    /// cloned candidate. Existing changed or removed entities must be editable;
    /// valid selection indices are retained and removed indices are discarded.
    pub fn commit_document(&mut self, mut candidate: DxfDocument) -> Result<bool, EditError> {
        candidate.solve_constraints()?;
        for (index, existing) in self.entities().iter().enumerate() {
            let changed = candidate.entities().get(index) != Some(existing)
                || self.entity_space(index) != candidate.entity_space(index);
            if changed {
                self.editable_layer(self.entity_layer(index).expect("existing entity layer"))?;
            }
        }
        let selection = self
            .selection()
            .iter()
            .copied()
            .filter(|index| *index < candidate.entities().len())
            .collect();
        self.checkpoint();
        self.install_document(candidate);
        self.select(selection);
        self.layer_visibility_restore = None;
        self.layer_previous_state = None;
        Ok(true)
    }

    /// Alias for callers that model the operation as replacing the editor's
    /// document rather than committing an external candidate.
    pub fn replace_document(&mut self, candidate: DxfDocument) -> Result<bool, EditError> {
        self.commit_document(candidate)
    }

    /// Replace paper-space layout state as one undoable editor transaction.
    pub fn set_layouts(&mut self, layouts: LayoutCollection) -> bool {
        let mut document = self.current.document.clone();
        document.set_layouts(layouts);
        self.commit_layout_document(document, self.selection().to_vec());
        true
    }

    pub fn create_layout(
        &mut self,
        name: &str,
        setup: crate::PageSetup,
    ) -> Result<bool, EditError> {
        if name.trim().is_empty()
            || name.chars().any(char::is_control)
            || self.canonical_layout_name(name).is_some()
        {
            return Err(crate::DxfError {
                line: 0,
                kind: crate::DxfErrorKind::UnsupportedProperty(2),
            }
            .into());
        }
        let mut document = self.current.document.clone();
        document.layouts_mut().create(name, setup)?;
        self.commit_layout_document(document, self.selection().to_vec());
        Ok(true)
    }

    pub fn activate_layout(&mut self, name: &str) -> Result<bool, EditError> {
        if self
            .current
            .layouts
            .active()
            .name()
            .eq_ignore_ascii_case(name)
        {
            return Ok(false);
        }
        let name = self
            .canonical_layout_name(name)
            .ok_or(GeometryError::UnsupportedPolylineConversion)?;
        let mut document = self.current.document.clone();
        document.layouts_mut().set_active(&name)?;
        self.commit_layout_document(document, self.selection().to_vec());
        Ok(true)
    }

    pub fn copy_layout(&mut self, source: &str, target: &str) -> Result<bool, EditError> {
        let source = self
            .canonical_layout_name(source)
            .ok_or(GeometryError::UnsupportedPolylineConversion)?;
        if source.eq_ignore_ascii_case("Model") || target.eq_ignore_ascii_case("Model") {
            return Err(crate::DxfError {
                line: 0,
                kind: crate::DxfErrorKind::UnsupportedProperty(2),
            }
            .into());
        }
        if target.trim().is_empty() || self.canonical_layout_name(target).is_some() {
            return Err(crate::DxfError {
                line: 0,
                kind: crate::DxfErrorKind::UnsupportedProperty(2),
            }
            .into());
        }
        for index in 0..self.entities().len() {
            let space = self.entity_space(index).expect("existing entity space");
            if space.matches_layout(&source) {
                let layer = self.entity_layer(index).expect("existing source layer");
                self.editable_layer(layer)?;
            }
        }
        let mut document = self.current.document.clone();
        document.copy_layout(&source, target)?;
        self.commit_layout_document(document, self.selection().to_vec());
        Ok(true)
    }

    pub fn rename_layout(&mut self, old: &str, new: &str) -> Result<bool, EditError> {
        let old = self
            .canonical_layout_name(old)
            .ok_or(GeometryError::UnsupportedPolylineConversion)?;
        if old.eq_ignore_ascii_case(new) {
            return Ok(false);
        }
        if self.canonical_layout_name(new).is_some() || new.trim().is_empty() {
            return Err(crate::DxfError {
                line: 0,
                kind: crate::DxfErrorKind::UnsupportedProperty(2),
            }
            .into());
        }
        let mut document = self.current.document.clone();
        document.rename_layout(&old, new)?;
        self.commit_layout_document(document, self.selection().to_vec());
        Ok(true)
    }

    pub fn delete_layout(&mut self, name: &str) -> Result<bool, EditError> {
        let name = self
            .canonical_layout_name(name)
            .ok_or(GeometryError::UnsupportedPolylineConversion)?;
        let removed: Vec<_> = (0..self.entities().len())
            .filter(|index| {
                self.entity_space(*index)
                    .is_some_and(|space| space.matches_layout(&name))
            })
            .collect();
        for &index in &removed {
            let layer = self
                .entity_layer(index)
                .expect("existing paper-space entity layer");
            self.editable_layer(layer)?;
        }
        let selected = self.selection().to_vec();
        let mut document = self.current.document.clone();
        document.delete_layout(&name)?;
        let selection = selected
            .into_iter()
            .filter(|index| !removed.contains(index))
            .map(|index| index - removed.iter().filter(|removed| **removed < index).count())
            .collect();
        self.commit_layout_document(document, selection);
        Ok(true)
    }

    fn commit_layout_document(&mut self, document: DxfDocument, selection: Vec<usize>) {
        self.checkpoint();
        self.install_document(document);
        self.select(selection);
    }

    /// Create a named group from the current selection as one undoable edit.
    pub fn create_group_from_selection(
        &mut self,
        name: impl Into<String>,
        description: impl Into<String>,
        selectable: bool,
    ) -> Result<bool, EditError> {
        if self.selection().is_empty() {
            return Err(crate::DxfError {
                line: 0,
                kind: crate::DxfErrorKind::MissingField(340),
            }
            .into());
        }
        self.editable_selection()?;
        let members = self.selection().to_vec();
        self.edit_document(|document| -> Result<bool, crate::DxfError> {
            document.create_group(name, description, selectable, &members)?;
            Ok(true)
        })
        .map_err(EditError::from)
    }

    /// Rename a group and retain the operation in the editor undo history.
    pub fn rename_group(
        &mut self,
        index: usize,
        name: impl Into<String>,
    ) -> Result<bool, EditError> {
        let name = name.into();
        self.edit_document(|document| -> Result<bool, crate::DxfError> {
            document.rename_group(index, name)?;
            Ok(true)
        })
        .map_err(EditError::from)
    }

    /// Replace a group's membership as one undoable edit.
    pub fn set_group_membership(
        &mut self,
        index: usize,
        members: &[usize],
    ) -> Result<bool, EditError> {
        self.edit_document(|document| -> Result<bool, crate::DxfError> {
            document.set_group_membership(index, members)?;
            Ok(true)
        })
        .map_err(EditError::from)
    }

    /// Remove a group as one undoable edit, returning whether it existed.
    pub fn remove_group(&mut self, index: usize) -> Result<bool, EditError> {
        self.edit_document(|document| -> Result<bool, crate::DxfError> {
            document.remove_group(index)?;
            Ok(true)
        })
        .map_err(EditError::from)
    }
    pub fn add_layer(&mut self, name: &str) -> Result<bool, LayerError> {
        self.edit_document(|document| {
            document.add_layer(name)?;
            Ok(true)
        })
    }
    pub fn set_layer_visible(&mut self, name: &str, visible: bool) -> Result<bool, LayerError> {
        let previous = self.layer_state_snapshot();
        let changed = self.edit_document(|document| document.set_layer_visible(name, visible))?;
        if changed && self.layer_previous_state.is_none() {
            self.layer_previous_state = Some(previous);
        }
        Ok(changed)
    }
    pub fn set_layer_locked(&mut self, name: &str, locked: bool) -> Result<bool, LayerError> {
        let previous = self.layer_state_snapshot();
        let changed = self.edit_document(|document| document.set_layer_locked(name, locked))?;
        if changed && self.layer_previous_state.is_none() {
            self.layer_previous_state = Some(previous);
        }
        Ok(changed)
    }
    pub fn set_layer_color(&mut self, name: &str, index: i16) -> Result<bool, LayerError> {
        self.edit_document(|document| document.set_layer_color(name, index))
    }
    pub fn add_linetype(&mut self, kind: Linetype) -> Result<bool, LinetypeError> {
        self.edit_document(|document| {
            document.add_linetype(kind)?;
            Ok(true)
        })
    }
    pub fn set_layer_linetype(&mut self, layer: &str, name: &str) -> Result<bool, LinetypeError> {
        self.edit_document(|document| document.set_layer_linetype(layer, name))
    }
    pub fn set_linetype_scale(&mut self, scale: f64) -> Result<bool, LinetypeError> {
        self.edit_document(|document| document.set_linetype_scale(scale))
    }
    pub fn corner_value(&self, parameter: CornerParameter) -> f64 {
        self.current.document.corner_value(parameter)
    }
    pub fn pline_width(&self) -> f64 {
        self.current.document.pline_width()
    }
    pub fn set_pline_width(&mut self, width: f64) -> Result<bool, GeometryError> {
        self.edit_document(|document| document.set_pline_width(width))
    }
    pub fn set_corner_value(
        &mut self,
        parameter: CornerParameter,
        value: f64,
    ) -> Result<bool, GeometryError> {
        self.set_corner_values(&[(parameter, value)])
    }
    pub fn set_corner_values(
        &mut self,
        values: &[(CornerParameter, f64)],
    ) -> Result<bool, GeometryError> {
        self.edit_document(|document| {
            let mut changed = false;
            for (parameter, value) in values {
                changed |= document.set_corner_value(*parameter, *value)?;
            }
            Ok(changed)
        })
    }
    pub fn assign_selection_layer(&mut self, name: &str) -> Result<bool, LayerError> {
        self.editable_selection()?;
        self.editable_layer(name)?;
        let selected = self.selection().to_vec();
        self.edit_document(|document| {
            let mut changed = false;
            for index in selected {
                changed |= document.set_entity_layer(index, name)?;
            }
            Ok(changed)
        })
    }

    /// Match all selected entities to a named layer in one undo transaction.
    pub fn match_selection_layer(&mut self, name: &str) -> Result<bool, LayerError> {
        self.assign_selection_layer(name)
    }

    fn layer_state_snapshot(&self) -> Vec<LayerState> {
        self.layers()
            .iter()
            .map(|layer| {
                (
                    layer.name().to_owned(),
                    layer.is_visible(),
                    layer.is_frozen(),
                    layer.is_locked(),
                    layer.is_off(),
                )
            })
            .collect()
    }

    /// Freeze a layer while preserving its off and lock state.
    pub fn freeze_layer(&mut self, name: &str) -> Result<bool, LayerError> {
        let previous = self.layer_state_snapshot();
        let changed = self.edit_document(|document| document.set_layer_frozen(name, true))?;
        if changed && self.layer_previous_state.is_none() {
            self.layer_previous_state = Some(previous);
        }
        Ok(changed)
    }

    pub fn thaw_layer(&mut self, name: &str) -> Result<bool, LayerError> {
        let previous = self.layer_state_snapshot();
        let changed = self.edit_document(|document| document.set_layer_frozen(name, false))?;
        if changed && self.layer_previous_state.is_none() {
            self.layer_previous_state = Some(previous);
        }
        Ok(changed)
    }

    /// Restore the layer state captured before the first LAYP-style edit.
    /// The restored state becomes the next baseline, allowing a second call
    /// to swap back without affecting the drawing undo stack.
    pub fn restore_previous_layer_state(&mut self) -> Result<bool, LayerError> {
        let Some(previous) = self.layer_previous_state.take() else {
            return Ok(false);
        };
        let current = self.layer_state_snapshot();
        let changed = self.edit_document(|document| {
            let mut changed = false;
            for (name, visible, frozen, locked, off) in &previous {
                if document.layer(name).is_none() {
                    continue;
                }
                changed |= document.set_layer_frozen(name, false)?;
                changed |= document.set_layer_locked(name, *locked)?;
                changed |= document.set_layer_visible(name, if *off { false } else { *visible })?;
                changed |= document.set_layer_frozen(name, *frozen)?;
            }
            Ok(changed)
        })?;
        if changed {
            self.layer_previous_state = Some(current);
        } else {
            self.layer_previous_state = Some(previous);
        }
        Ok(changed)
    }

    /// Show one layer and hide the other layers, retaining their prior
    /// visibility for `restore_layer_visibility`.
    pub fn isolate_layer(&mut self, name: &str) -> Result<bool, LayerError> {
        let target = self
            .current
            .document
            .layer(name)
            .ok_or_else(|| LayerError::UnknownLayer(name.into()))?
            .name()
            .to_owned();
        let previous = self.layer_state_snapshot();
        let changed = self.edit_document(|document| {
            let mut changed = false;
            for layer in previous.iter().map(|(name, ..)| name) {
                changed |=
                    document.set_layer_visible(layer, layer.eq_ignore_ascii_case(&target))?;
            }
            Ok(changed)
        })?;
        if changed {
            self.layer_visibility_restore = Some(previous);
            if self.layer_previous_state.is_none() {
                self.layer_previous_state = self.layer_visibility_restore.clone();
            }
        }
        Ok(changed)
    }

    pub fn restore_layer_visibility(&mut self) -> Result<bool, LayerError> {
        let Some(previous) = self.layer_visibility_restore.take() else {
            return Ok(false);
        };
        let changed = self.edit_document(|document| {
            let mut changed = false;
            for (name, visible, frozen, locked, off) in &previous {
                if document.layer(name).is_none() {
                    continue;
                }
                changed |= document.set_layer_frozen(name, false)?;
                changed |= document.set_layer_locked(name, *locked)?;
                changed |= document.set_layer_visible(name, if *off { false } else { *visible })?;
                changed |= document.set_layer_frozen(name, *frozen)?;
            }
            Ok(changed)
        })?;
        if !changed {
            self.layer_visibility_restore = Some(previous);
        }
        Ok(changed)
    }

    pub fn unisolate_layers(&mut self) -> Result<bool, LayerError> {
        self.restore_layer_visibility()
    }
    fn edit_selection_style(
        &mut self,
        mut edit: impl FnMut(&mut DxfDocument, usize) -> Result<bool, EntityStyleError>,
    ) -> Result<bool, EditError> {
        if self.selection().is_empty() {
            return Err(EntityStyleError::NoSelection.into());
        }
        self.editable_selection()?;
        let selected = self.selection().to_vec();
        self.edit_document::<EntityStyleError>(|document| {
            let mut changed = false;
            for index in selected {
                changed |= edit(document, index)?;
            }
            Ok(changed)
        })
        .map_err(EditError::from)
    }
    pub fn assign_selection_color(&mut self, color: i16) -> Result<bool, EditError> {
        self.edit_selection_style(|document, index| document.set_entity_color(index, color))
    }
    pub fn assign_selection_linetype(&mut self, name: &str) -> Result<bool, EditError> {
        self.edit_selection_style(|document, index| document.set_entity_linetype(index, name))
    }
    pub fn assign_selection_linetype_scale(&mut self, scale: f64) -> Result<bool, EditError> {
        self.edit_selection_style(|document, index| {
            document.set_entity_linetype_scale(index, scale)
        })
    }
    pub fn is_dirty(&self) -> bool {
        self.current.revision != self.saved_revision
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn mark_saved(&mut self) {
        self.saved_revision = self.current.revision;
    }
    pub fn to_dxf(&self) -> String {
        self.current.document.to_dxf()
    }

    pub fn to_svg(&self) -> Result<String, crate::SvgError> {
        crate::render_document_svg(&self.current.document)
    }

    pub fn select(&mut self, mut indices: Vec<usize>) {
        indices.retain(|index| self.is_entity_visible(*index));
        indices.sort_unstable();
        indices.dedup();
        self.current.selection = indices;
    }

    pub fn toggle_selection(&mut self, index: usize) {
        let mut indices = self.current.selection.clone();
        if let Some(position) = indices.iter().position(|selected| *selected == index) {
            indices.remove(position);
        } else {
            indices.push(index);
        }
        self.select(indices);
    }

    fn checkpoint(&mut self) {
        self.undo.push(self.current.clone());
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.current.revision = self.next_revision;
        self.next_revision += 1;
    }

    fn install_document(&mut self, document: DxfDocument) {
        self.current.layouts = document.layouts().clone();
        self.current.document = document;
    }

    pub fn add(&mut self, entities: Vec<Entity>) -> Result<bool, LayerError> {
        if entities.is_empty() {
            return Ok(false);
        }
        self.editable_layer(&self.active_layer)?;
        let mut candidate = self.current.document.clone();
        let space = self.active_space();
        let first = candidate.entities().len();
        for entity in entities {
            candidate
                .push_with_style_in_space(
                    entity,
                    &self.active_layer,
                    EntityStyle::default(),
                    space.clone(),
                )
                .expect("validated current layer");
        }
        let new_selection = (first..candidate.entities().len()).collect();
        self.commit_geometry_candidate(candidate, new_selection)
            .map_err(Self::geometry_commit_layer_error)
    }

    /// Replace the current selection with zero or more entities in one undo
    /// transaction. New entities inherit the first selected entity's layer and
    /// explicit style; unrelated records retain their existing metadata.
    pub fn replace_selection_with_entities(
        &mut self,
        entities: Vec<Entity>,
    ) -> Result<bool, EditError> {
        let mut selected = self.selection().to_vec();
        if selected.is_empty() {
            return Ok(false);
        }
        selected.sort_unstable();
        selected.dedup();
        for &index in &selected {
            let layer = self
                .entity_layer(index)
                .ok_or(LayerError::MissingEntity(index))?;
            self.editable_layer(layer)?;
        }
        let layer = self
            .entity_layer(selected[0])
            .ok_or(LayerError::MissingEntity(selected[0]))?
            .to_owned();
        let style = self
            .entity_style(selected[0])
            .cloned()
            .ok_or(EntityStyleError::UnknownEntity(selected[0]))?;
        let space = self
            .entity_space(selected[0])
            .cloned()
            .expect("existing selected entity space");
        let mut candidate = self.current.document.clone();
        for index in selected.iter().rev() {
            candidate.remove(*index);
        }
        let first = candidate.entities().len();
        for entity in entities {
            candidate.push_with_style_in_space(entity, &layer, style.clone(), space.clone())?;
        }
        let new_selection = (first..candidate.entities().len()).collect();
        self.commit_geometry_candidate(candidate, new_selection)
    }

    /// Define a named block from the current selection. Geometry is stored in
    /// local coordinates relative to `base`; optionally replace the selection
    /// with one INSERT in the same transaction.
    pub fn define_block(
        &mut self,
        name: &str,
        base: crate::Point3,
        replace_selection_with_insert: bool,
    ) -> Result<bool, EditError> {
        if name.trim().is_empty() || self.current.document.block(name).is_some() {
            return Err(crate::DxfError {
                line: 0,
                kind: crate::DxfErrorKind::UnsupportedProperty(2),
            }
            .into());
        }
        let selected = self.selection().to_vec();
        if selected.is_empty() {
            return Ok(false);
        }
        self.editable_selection()?;
        let local = selected
            .iter()
            .map(|index| self.entities()[*index].translated(-base.x(), -base.y(), -base.z()))
            .collect::<Result<Vec<_>, _>>()?;
        let definition = crate::BlockDefinition::new(name, base, local);
        let mut candidate = self.current.document.clone();
        candidate.add_block_definition(definition)?;
        let mut new_selection = Vec::new();
        if replace_selection_with_insert {
            let source = selected[0];
            let layer = self
                .entity_layer(source)
                .expect("selected source layer")
                .to_owned();
            self.editable_layer(&layer)?;
            let style = self
                .entity_style(source)
                .cloned()
                .expect("selected source style");
            let space = self
                .entity_space(source)
                .cloned()
                .expect("selected source space");
            for index in selected.iter().rev() {
                candidate.remove(*index);
            }
            let insert = crate::Insert::new(name, base);
            let index = candidate.entities().len();
            candidate
                .push_with_style_in_space(crate::Entity::Insert(insert), &layer, style, space)
                .expect("validated current layer");
            new_selection.push(index);
        } else {
            new_selection = selected;
        }
        self.checkpoint();
        self.install_document(candidate);
        self.select(new_selection);
        Ok(true)
    }

    /// Import one externally loaded block definition atomically.  The
    /// definition is registered without adding entities or changing the
    /// current selection; duplicate names are rejected by the document API.
    pub fn import_block_definition(
        &mut self,
        definition: BlockDefinition,
    ) -> Result<bool, EditError> {
        let mut document = self.current.document.clone();
        document.add_block_definition(definition)?;
        self.checkpoint();
        self.install_document(document);
        Ok(true)
    }

    /// Replace a named definition while preserving its registry position.
    /// Linked INSERT layers are validated before the transaction is recorded.
    pub fn replace_block_definition(
        &mut self,
        definition: BlockDefinition,
    ) -> Result<bool, EditError> {
        let Some(existing) = self.current.document.block(definition.name()) else {
            return Err(crate::DxfError {
                line: 0,
                kind: crate::DxfErrorKind::UnsupportedProperty(2),
            }
            .into());
        };
        if existing == &definition {
            return Ok(false);
        }
        let linked_layers: Vec<String> = self
            .entities()
            .iter()
            .enumerate()
            .filter(|(_, entity)| {
                matches!(entity, Entity::Insert(insert) if insert.block_name().eq_ignore_ascii_case(definition.name()))
            })
            .map(|(index, _)| {
                self.entity_layer(index)
                    .expect("existing insert layer")
                    .to_owned()
            })
            .collect();
        for layer in linked_layers {
            self.editable_layer(&layer)?;
        }
        let mut document = self.current.document.clone();
        document.replace_block_definition(definition)?;
        self.checkpoint();
        self.install_document(document);
        Ok(true)
    }

    /// Insert a validated named block on the current layer as one undo step.
    pub fn insert_block(
        &mut self,
        name: &str,
        insertion: crate::Point3,
        scale: [f64; 3],
        rotation: f64,
        array: Option<(usize, usize, f64, f64)>,
    ) -> Result<bool, EditError> {
        let definition = self.current.document.block(name).ok_or(crate::DxfError {
            line: 0,
            kind: crate::DxfErrorKind::UnsupportedProperty(2),
        })?;
        let _ = definition;
        let mut insert = crate::Insert::new(name, insertion);
        insert.set_scale(scale)?;
        insert.set_rotation(rotation)?;
        if let Some((rows, columns, row_spacing, column_spacing)) = array {
            if rows == 0
                || columns == 0
                || rows.checked_mul(columns).is_none_or(|count| count > 512)
            {
                return Err(GeometryError::NonFiniteParameter.into());
            }
            insert.set_array(Some(rows), Some(columns), row_spacing, column_spacing)?;
        }
        self.add(vec![crate::Entity::Insert(insert)])
            .map_err(EditError::from)
    }

    /// Replace an INSERT's attribute values atomically while retaining its
    /// layer, style, placement, and block reference.
    pub fn set_insert_attributes(
        &mut self,
        index: usize,
        attributes: Vec<crate::Attribute>,
    ) -> Result<bool, EditError> {
        let Some(crate::Entity::Insert(insert)) = self.entities().get(index) else {
            return Err(GeometryError::UnsupportedPolylineConversion.into());
        };
        let mut replacement = insert.clone();
        replacement.set_attributes(attributes);
        if replacement == *insert {
            return Ok(false);
        }
        self.editable_layer(self.entity_layer(index).expect("existing insert layer"))?;
        self.checkpoint();
        self.current
            .document
            .replace_entity(index, crate::Entity::Insert(replacement))
            .expect("same entity type");
        self.select(vec![index]);
        Ok(true)
    }

    /// Add one attribute definition to an existing block in one undo step.
    pub fn add_block_attribute_definition(
        &mut self,
        block: &str,
        definition: crate::AttributeDefinition,
    ) -> Result<bool, EditError> {
        let Some(existing) = self.current.document.block(block) else {
            return Err(crate::DxfError {
                line: 0,
                kind: crate::DxfErrorKind::UnsupportedProperty(2),
            }
            .into());
        };
        let mut replacement = existing.clone();
        if replacement
            .attribute_definitions()
            .iter()
            .any(|item| item.tag().eq_ignore_ascii_case(definition.tag()))
        {
            return Ok(false);
        }
        let insert_layers: Vec<String> = self
            .entities()
            .iter()
            .enumerate()
            .filter(|(_, entity)| {
                matches!(entity, crate::Entity::Insert(insert) if insert.block_name().eq_ignore_ascii_case(block))
            })
            .map(|(index, _)| self.entity_layer(index).expect("existing insert layer").to_owned())
            .collect();
        for layer in insert_layers {
            self.editable_layer(&layer)?;
        }
        replacement.add_attribute_definition(definition);
        self.checkpoint();
        self.current
            .document
            .replace_block_definition(replacement)?;
        Ok(true)
    }

    pub fn update(&mut self, index: usize, entity: Entity) -> Result<bool, LayerError> {
        let Some(old) = self.entities().get(index) else {
            return Ok(false);
        };
        if *old == entity || old.name() != entity.name() {
            return Ok(false);
        }
        self.editable_layer(self.entity_layer(index).expect("existing entity layer"))?;
        let mut candidate = self.current.document.clone();
        candidate
            .replace_entity(index, entity)
            .expect("validated existing entity type");
        self.commit_geometry_candidate(candidate, self.selection().to_vec())
            .map_err(Self::geometry_commit_layer_error)
    }

    /// Apply validated replacements to the current selection in one undo step.
    pub fn update_selection_atomic(
        &mut self,
        replacements: Vec<(usize, Entity)>,
    ) -> Result<bool, EditError> {
        if replacements.is_empty() {
            return Ok(false);
        }
        let mut candidate = self.current.document.clone();
        for (index, entity) in replacements {
            if self.entities().get(index).is_none() {
                return Err(LayerError::MissingEntity(index).into());
            }
            candidate.replace_entity(index, entity);
        }
        self.commit_geometry_candidate(candidate, self.selection().to_vec())
    }

    pub fn translate_selection(
        &mut self,
        dx: f64,
        dy: f64,
        dz: f64,
        copy: bool,
    ) -> Result<bool, EditError> {
        self.map_selection(copy, |entity| entity.translated(dx, dy, dz))
    }

    /// Transform a selection as one undo step, preserving source records and styles.
    pub fn transform_selection(
        &mut self,
        transform: Transform,
        copy: bool,
    ) -> Result<bool, EditError> {
        self.map_selection(copy, |entity| entity.transformed(transform))
    }

    pub fn change_selection_space(
        &mut self,
        target: EntitySpace,
        transform: Transform,
    ) -> Result<bool, EditError> {
        let selected = self.selection().to_vec();
        self.change_entities_space(&selected, target, transform)
    }

    /// Move explicit source indices into a layout space after a validated transform.
    /// Explicit indices let CHSPACE transfer model entities while a paper layout is active.
    pub fn change_entities_space(
        &mut self,
        indices: &[usize],
        target: EntitySpace,
        transform: Transform,
    ) -> Result<bool, EditError> {
        if indices.is_empty() {
            return Ok(false);
        }
        let target = self.canonical_entity_space(target)?;
        let replacements = self.plan_space_changes(indices, &target, transform)?;
        if replacements.is_empty() {
            return Ok(false);
        }
        let selection = replacements.iter().map(|(index, _)| *index).collect();
        let mut candidate = self.current.document.clone();
        for (index, entity) in replacements {
            candidate
                .replace_entity(index, entity)
                .expect("space transform preserves entity type and handle");
            candidate.set_entity_space(index, target.clone())?;
        }
        self.commit_geometry_candidate(candidate, selection)
    }

    fn plan_space_changes(
        &self,
        indices: &[usize],
        target: &EntitySpace,
        transform: Transform,
    ) -> Result<Vec<(usize, Entity)>, EditError> {
        let mut indices = indices.to_vec();
        indices.sort_unstable();
        indices.dedup();
        let mut replacements = Vec::with_capacity(indices.len());
        for index in indices {
            let source = self
                .entities()
                .get(index)
                .ok_or(LayerError::MissingEntity(index))?;
            let layer = self.entity_layer(index).expect("existing source layer");
            self.editable_layer(layer)?;
            let transformed = source.transformed(transform)?;
            let source_space = self.entity_space(index).expect("existing source space");
            if transformed != *source || source_space != target {
                replacements.push((index, transformed));
            }
        }
        Ok(replacements)
    }

    /// Apply a geometry operation to the current selection in one transaction.
    /// The operation is evaluated completely before the document is changed.
    pub fn map_selection_with(
        &mut self,
        copy: bool,
        operation: impl Fn(&Entity) -> Result<Entity, GeometryError>,
    ) -> Result<bool, EditError> {
        self.map_selection(copy, operation)
    }

    fn geometry_commit_layer_error(error: EditError) -> LayerError {
        match error {
            EditError::Layer(layer) => layer,
            EditError::Constraint(constraint) => LayerError::Constraint(constraint.to_string()),
            other => LayerError::Constraint(other.to_string()),
        }
    }

    fn commit_geometry_candidate(
        &mut self,
        mut candidate: DxfDocument,
        selection: Vec<usize>,
    ) -> Result<bool, EditError> {
        candidate.solve_constraints()?;
        let changed = self.entities() != candidate.entities()
            || (0..self.entities().len().min(candidate.entities().len()))
                .any(|index| self.entity_space(index) != candidate.entity_space(index));
        if !changed {
            return Ok(false);
        }
        for (index, (before, after)) in self.entities().iter().zip(candidate.entities()).enumerate()
        {
            if before != after || self.entity_space(index) != candidate.entity_space(index) {
                self.editable_layer(
                    self.entity_layer(index)
                        .ok_or(LayerError::MissingEntity(index))?,
                )?;
            }
        }
        self.checkpoint();
        self.install_document(candidate);
        self.select(selection);
        Ok(true)
    }

    fn map_selection(
        &mut self,
        copy: bool,
        operation: impl Fn(&Entity) -> Result<Entity, GeometryError>,
    ) -> Result<bool, EditError> {
        if self.selection().is_empty() {
            return Ok(false);
        }
        self.editable_selection()?;
        let transformed = self
            .selection()
            .iter()
            .map(|index| operation(&self.entities()[*index]))
            .collect::<Result<Vec<_>, _>>()?;
        let mut candidate = self.current.document.clone();
        if copy {
            let styles: Vec<_> = self
                .selection()
                .iter()
                .map(|index| {
                    (
                        self.entity_layer(*index)
                            .expect("valid selection")
                            .to_owned(),
                        self.entity_style(*index).expect("valid selection").clone(),
                        self.entity_space(*index)
                            .expect("valid selection space")
                            .clone(),
                    )
                })
                .collect();
            let first = candidate.entities().len();
            for (entity, (layer, style, space)) in transformed.into_iter().zip(styles) {
                candidate
                    .push_with_style_in_space(entity, &layer, style, space)
                    .expect("existing source layer");
            }
            let new_selection = (first..candidate.entities().len()).collect();
            return self.commit_geometry_candidate(candidate, new_selection);
        }
        if transformed
            .iter()
            .zip(self.selection())
            .all(|(entity, index)| *entity == self.entities()[*index])
        {
            return Ok(false);
        }
        for (index, entity) in self.current.selection.iter().copied().zip(transformed) {
            candidate
                .replace_entity(index, entity)
                .expect("transform preserves type");
        }
        self.commit_geometry_candidate(candidate, self.selection().to_vec())
    }

    /// Add one transformed copy of every selected entity for each transform.
    /// Source layer and explicit style are retained; all copies form one undo step.
    pub fn copy_selection_many(&mut self, transforms: &[Transform]) -> Result<bool, EditError> {
        if self.selection().is_empty() || transforms.is_empty() {
            return Ok(false);
        }
        let count = self
            .selection()
            .len()
            .checked_mul(transforms.len())
            .ok_or(GeometryError::NonFiniteResult)?;
        if count > 2048 {
            return Err(GeometryError::NonFiniteResult.into());
        }
        self.editable_selection()?;
        let sources: Vec<_> = self
            .selection()
            .iter()
            .map(|index| {
                let entity = self.entities()[*index].clone();
                let layer = self
                    .entity_layer(*index)
                    .expect("valid selection")
                    .to_owned();
                let style = self.entity_style(*index).expect("valid selection").clone();
                let space = self
                    .entity_space(*index)
                    .expect("valid selection space")
                    .clone();
                Ok((entity, layer, style, space))
            })
            .collect::<Result<_, EditError>>()?;
        let mut copies = Vec::with_capacity(count);
        for transform in transforms {
            for (entity, layer, style, space) in &sources {
                copies.push((
                    entity.transformed(*transform)?,
                    layer.clone(),
                    style.clone(),
                    space.clone(),
                ));
            }
        }
        let mut candidate = self.current.document.clone();
        let first = candidate.entities().len();
        for (entity, layer, style, space) in copies {
            candidate
                .push_with_style_in_space(entity, &layer, style, space)
                .expect("validated source layer");
        }
        let new_selection = (first..candidate.entities().len()).collect();
        self.commit_geometry_candidate(candidate, new_selection)
    }

    /// Copy all layer and explicit style properties from one entity to targets.
    pub fn match_properties(
        &mut self,
        source: usize,
        targets: &[usize],
    ) -> Result<bool, EditError> {
        let source_layer = self
            .entity_layer(source)
            .ok_or(LayerError::UnknownLayer(source.to_string()))?
            .to_owned();
        let source_style = self
            .entity_style(source)
            .ok_or(EntityStyleError::NoSelection)?
            .clone();
        let mut targets = targets.to_vec();
        targets.sort_unstable();
        targets.dedup();
        for &target in &targets {
            if target >= self.entities().len() {
                return Err(LayerError::UnknownLayer(target.to_string()).into());
            }
            self.editable_layer(self.entity_layer(target).expect("valid target"))?;
            self.editable_layer(&source_layer)?;
        }
        if targets.is_empty() {
            return Ok(false);
        }
        let mut changed = false;
        for &target in &targets {
            changed |= self
                .entity_layer(target)
                .is_some_and(|layer| layer != source_layer)
                || self.entity_style(target) != Some(&source_style);
        }
        if !changed {
            return Ok(false);
        }
        self.checkpoint();
        for target in targets {
            self.current
                .document
                .set_entity_layer(target, &source_layer)?;
            self.current
                .document
                .set_entity_color(target, source_style.color_index())?;
            self.current
                .document
                .set_entity_linetype(target, source_style.linetype())?;
            self.current
                .document
                .set_entity_linetype_scale(target, source_style.linetype_scale())?;
        }
        Ok(true)
    }

    /// Create an offset and optionally erase its source in the same undo step.
    /// Explicit entity style overrides are copied; BYLAYER resolves on the target layer.
    pub fn offset_entity(
        &mut self,
        index: usize,
        offset: Offset,
        options: OffsetOptions,
    ) -> Result<bool, EditError> {
        let Some(source) = self.entities().get(index) else {
            return Ok(false);
        };
        let entity = source.offset_xy(offset)?;
        let source_layer = self.entity_layer(index).expect("existing source layer");
        self.editable_layer(source_layer)?;
        let target_layer = match options.layer {
            OffsetLayer::Source => source_layer,
            OffsetLayer::Current => &self.active_layer,
        }
        .to_owned();
        self.editable_layer(&target_layer)?;
        let style = self
            .entity_style(index)
            .expect("existing source style")
            .clone();
        let space = self
            .entity_space(index)
            .expect("existing source space")
            .clone();
        let mut candidate = self.current.document.clone();
        if options.erase_source {
            candidate.remove(index).expect("validated source index");
        }
        let created = candidate.entities().len();
        candidate
            .push_with_style_in_space(entity, &target_layer, style, space)
            .expect("validated target layer");
        self.commit_geometry_candidate(candidate, vec![created])
    }

    /// Trim or split one source in a single transaction, retaining its layer/style.
    /// Same-type first fragments retain their source record; converted circles
    /// and additional fragments become new records.
    pub fn trim_entity(
        &mut self,
        index: usize,
        boundaries: &[usize],
        pick: crate::Point3,
        mode: BoundaryMode,
    ) -> Result<bool, EditError> {
        let Some(source) = self.entities().get(index) else {
            return Ok(false);
        };
        let edges = self.cutting_edges(index, boundaries);
        let parts = source.trim_xy(&edges, pick, mode)?;
        self.replace_curve_parts(index, parts)
    }

    /// Break one curve in a single transaction. Same-type first fragments keep
    /// the source record; additional/converted fragments copy its layer/style.
    pub fn break_entity(
        &mut self,
        index: usize,
        first: crate::Point3,
        second: crate::Point3,
    ) -> Result<bool, EditError> {
        let Some(source) = self.entities().get(index) else {
            return Ok(false);
        };
        let parts = source.break_xy(first, second)?;
        self.replace_curve_parts(index, parts)
    }

    pub fn break_entity_at(
        &mut self,
        index: usize,
        pick: crate::Point3,
    ) -> Result<bool, EditError> {
        let Some(source) = self.entities().get(index) else {
            return Ok(false);
        };
        let parts = source.break_at_xy(pick)?;
        self.replace_curve_parts(index, parts)
    }

    /// Break a polyline at vertex indices in one protected transaction.
    /// The first fragment keeps the source record; others inherit layer/style.
    pub fn break_polyline_vertices(
        &mut self,
        index: usize,
        first: usize,
        second: usize,
    ) -> Result<bool, EditError> {
        let Some(source) = self.entities().get(index) else {
            return Ok(false);
        };
        let Entity::Polyline(polyline) = source else {
            return Err(GeometryError::UnsupportedPolylineBreak.into());
        };
        let parts = polyline
            .break_vertices(first, second)?
            .into_iter()
            .map(Entity::Polyline)
            .collect();
        self.replace_curve_parts(index, parts)
    }

    /// Change one endpoint in place, retaining source identity/style and one undo step.
    pub fn lengthen_entity(
        &mut self,
        index: usize,
        pick: crate::Point3,
        operation: Lengthen,
    ) -> Result<bool, EditError> {
        let Some(source) = self.entities().get(index) else {
            return Ok(false);
        };
        let entity = source.lengthen_xy(pick, operation)?;
        self.replace_curve_parts(index, vec![entity])
    }

    pub fn extend_entity(
        &mut self,
        index: usize,
        boundaries: &[usize],
        pick: crate::Point3,
        mode: BoundaryMode,
    ) -> Result<bool, EditError> {
        let Some(source) = self.entities().get(index) else {
            return Ok(false);
        };
        let edges = self.cutting_edges(index, boundaries);
        let entity = source.extend_xy(&edges, pick, mode)?;
        self.replace_curve_parts(index, vec![entity])
    }

    /// Construct a line pair and its connector as one drawing transaction.
    /// Retained source records keep their styles and identities. A connector
    /// uses BYLAYER defaults on their common layer, or the current layer.
    pub fn corner_lines(
        &mut self,
        picks: [(usize, crate::Point3); 2],
        corner: Corner,
        trim: bool,
    ) -> Result<bool, EditError> {
        let [(first, first_pick), (second, second_pick)] = picks;
        if first == second {
            return Err(GeometryError::UnsupportedCornerEntities.into());
        }
        let (Some(Entity::Line(a)), Some(Entity::Line(b))) =
            (self.entities().get(first), self.entities().get(second))
        else {
            return Err(GeometryError::UnsupportedCornerEntities.into());
        };
        let result = corner.between_lines(*a, first_pick, *b, second_pick)?;
        let changed =
            trim && (result.first() != *a || result.second() != *b) || result.connector().is_some();
        if !changed {
            return Ok(false);
        }
        let first_layer = self.entity_layer(first).expect("existing source layer");
        let second_layer = self.entity_layer(second).expect("existing source layer");
        if trim {
            self.editable_layer(first_layer)?;
            self.editable_layer(second_layer)?;
        } else if !self.is_entity_visible(first) || !self.is_entity_visible(second) {
            return Err(LayerError::HiddenLayer(
                if !self.is_entity_visible(first) {
                    first_layer
                } else {
                    second_layer
                }
                .into(),
            )
            .into());
        }
        let connector_layer = if first_layer.eq_ignore_ascii_case(second_layer) {
            first_layer
        } else {
            self.current_layer()
        }
        .to_owned();
        if result.connector().is_some() {
            self.editable_layer(&connector_layer)?;
        }
        let mut candidate = self.current.document.clone();
        let space = self
            .entity_space(first)
            .expect("existing first source space")
            .clone();
        if trim {
            candidate
                .replace_entity(first, Entity::Line(result.first()))
                .expect("validated source line");
            candidate
                .replace_entity(second, Entity::Line(result.second()))
                .expect("validated source line");
        }
        let mut selected = vec![first, second];
        if let Some(connector) = result.connector() {
            selected.push(candidate.entities().len());
            candidate
                .push_with_style_in_space(
                    connector.clone(),
                    &connector_layer,
                    EntityStyle::default(),
                    space,
                )
                .expect("validated connector layer");
        }
        self.commit_geometry_candidate(candidate, selected)
    }

    fn cutting_edges(&self, source: usize, boundaries: &[usize]) -> Vec<Entity> {
        boundaries
            .iter()
            .copied()
            .filter(|index| *index != source && self.is_entity_visible(*index))
            .filter_map(|index| self.entities().get(index).cloned())
            .collect()
    }

    fn replace_curve_parts(
        &mut self,
        index: usize,
        mut parts: Vec<Entity>,
    ) -> Result<bool, EditError> {
        let source = &self.entities()[index];
        if parts.as_slice() == std::slice::from_ref(source) {
            return Ok(false);
        }
        let layer = self
            .entity_layer(index)
            .expect("existing source layer")
            .to_owned();
        self.editable_layer(&layer)?;
        let style = self
            .entity_style(index)
            .expect("existing source style")
            .clone();
        let space = self
            .entity_space(index)
            .expect("existing source space")
            .clone();
        let same_type = parts
            .first()
            .is_some_and(|part| part.name() == source.name());
        let mut candidate = self.current.document.clone();
        let mut selected = Vec::new();
        if same_type {
            candidate
                .replace_entity(index, parts.remove(0))
                .expect("validated source type");
            selected.push(index);
        } else {
            candidate.remove(index).expect("validated source index");
        }
        for part in parts {
            selected.push(candidate.entities().len());
            candidate
                .push_with_style_in_space(part, &layer, style.clone(), space.clone())
                .expect("validated source layer");
        }
        self.commit_geometry_candidate(candidate, selected)
    }

    /// Replace selected polylines and block INSERTs with their components in one edit.
    /// All fallible work precedes the checkpoint; output records inherit source layer/style.
    pub fn explode_selection(&mut self) -> Result<usize, EditError> {
        let mut plans = Vec::new();
        for &index in self.selection() {
            let layer = self.entity_layer(index).expect("existing source layer");
            self.editable_layer(layer)?;
            let parts = match &self.entities()[index] {
                Entity::Polyline(polyline) => polyline.explode()?,
                Entity::Insert(insert) => {
                    crate::expand_entity(&self.current.document, &Entity::Insert(insert.clone()))?
                }
                _ => continue,
            };
            let style = self.entity_style(index).expect("existing source style");
            let space = self
                .entity_space(index)
                .expect("existing source space")
                .clone();
            plans.push((index, layer.to_owned(), style.clone(), space, parts));
        }
        let count = plans.len();
        if count == 0 {
            return Ok(0);
        }
        let mut candidate = self.current.document.clone();
        for (index, ..) in plans.iter().rev() {
            candidate.remove(*index).expect("validated source index");
        }
        let first = candidate.entities().len();
        for (_, layer, style, space, parts) in plans {
            for part in parts {
                candidate
                    .push_with_style_in_space(part, &layer, style.clone(), space.clone())
                    .expect("validated source layer");
            }
        }
        let new_selection = (first..candidate.entities().len()).collect();
        self.commit_geometry_candidate(candidate, new_selection)?;
        Ok(count)
    }

    /// Merge visible compatible candidates into one source, retaining its
    /// record/layer/style. Incompatible objects are ignored. All consumed
    /// layers and geometry are validated before one history checkpoint.
    pub fn join_entities(
        &mut self,
        source: usize,
        candidates: &[usize],
    ) -> Result<usize, EditError> {
        let Some(entity) = self.entities().get(source) else {
            return Ok(0);
        };
        entity.join_xy(&[])?;
        let mut consumed = Vec::new();
        for &index in candidates {
            if index == source || !self.is_entity_visible(index) || consumed.contains(&index) {
                continue;
            }
            if let Some(candidate) = self.entities().get(index)
                && entity.join_compatible_xy(candidate)?
            {
                consumed.push(index);
            }
        }
        if consumed.is_empty() {
            return Ok(0);
        }
        let others: Vec<_> = consumed
            .iter()
            .map(|index| self.entities()[*index].clone())
            .collect();
        let result = entity.join_xy(&others)?;
        self.commit_join(source, result, consumed)
    }

    /// Join touching, same-elevation LINE/ARC/open POLYLINE candidates to an
    /// open polyline. Document order resolves branches; repeated scans accept
    /// chains regardless of selection order. One checkpoint covers the merge.
    pub fn join_polyline_entities(
        &mut self,
        source: usize,
        candidates: &[usize],
    ) -> Result<usize, EditError> {
        let Some(entity) = self.entities().get(source) else {
            return Ok(0);
        };
        let Entity::Polyline(polyline) = entity else {
            return Err(GeometryError::UnsupportedPolylineConversion.into());
        };
        if polyline.is_closed() {
            return Err(GeometryError::ClosedPolylineJoin.into());
        }
        polyline.length()?;
        polyline.extrema()?;
        let mut result = polyline.clone();
        let mut pending: Vec<_> = candidates
            .iter()
            .copied()
            .filter(|index| *index != source && self.is_entity_visible(*index))
            .collect();
        pending.sort_unstable();
        pending.dedup();
        let mut consumed = Vec::new();
        loop {
            let before = consumed.len();
            for &index in &pending {
                if consumed.contains(&index) {
                    continue;
                }
                if let Some(joined) = result.join_endpoints_xy(&self.entities()[index])? {
                    result = joined;
                    consumed.push(index);
                }
            }
            if consumed.len() == before {
                break;
            }
        }
        if consumed.is_empty() {
            return Ok(0);
        }
        self.commit_join(source, Entity::Polyline(result), consumed)
    }

    fn commit_join(
        &mut self,
        source: usize,
        result: Entity,
        mut consumed: Vec<usize>,
    ) -> Result<usize, EditError> {
        for index in std::iter::once(&source).chain(consumed.iter()) {
            self.editable_layer(self.entity_layer(*index).expect("validated join index"))?;
        }
        consumed.sort_unstable();
        let mut candidate = self.current.document.clone();
        candidate
            .replace_entity(source, result)
            .expect("same join source type");
        for &index in consumed.iter().rev() {
            candidate.remove(index).expect("validated join candidate");
        }
        let selection = vec![source - consumed.iter().filter(|index| **index < source).count()];
        self.commit_geometry_candidate(candidate, selection)?;
        Ok(consumed.len())
    }

    pub fn close_arc_entity(&mut self, index: usize) -> Result<bool, EditError> {
        let Some(source) = self.entities().get(index) else {
            return Ok(false);
        };
        let circle = source.close_arc_xy()?;
        self.replace_curve_parts(index, vec![circle])
    }

    /// Replace a line/arc with one polyline in a single history transaction.
    /// Converted records are new and inherit the source layer/style.
    pub fn convert_to_polyline(&mut self, index: usize) -> Result<bool, EditError> {
        let Some(source) = self.entities().get(index) else {
            return Ok(false);
        };
        let polyline = source.to_polyline_xy()?;
        self.replace_curve_parts(index, vec![Entity::Polyline(polyline)])
    }

    pub fn delete_selection(&mut self) -> Result<bool, EditError> {
        if self.selection().is_empty() {
            return Ok(false);
        }
        self.editable_selection()?;
        let mut candidate = self.current.document.clone();
        for index in self.current.selection.iter().rev().copied() {
            candidate
                .remove(index)
                .expect("selection contains existing entities");
        }
        self.commit_geometry_candidate(candidate, Vec::new())
    }

    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.undo.pop() else {
            return false;
        };
        self.redo
            .push(std::mem::replace(&mut self.current, previous));
        if self.current.document.layer(&self.active_layer).is_none() {
            self.active_layer = "0".into();
        }
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };
        self.undo.push(std::mem::replace(&mut self.current, next));
        if self.current.document.layer(&self.active_layer).is_none() {
            self.active_layer = "0".into();
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Circle3, Line3, Point3};

    #[test]
    fn layer_color_history_retains_visibility_selection_and_saved_revision() {
        let source = include_str!("../tests/fixtures/layers.dxf");
        let mut editor = Editor::new(DxfDocument::parse(source).unwrap());
        editor.select(vec![0, 3]);
        assert!(!editor.set_layer_color("LOCKED", 2).unwrap());
        assert_eq!(
            editor.set_layer_color("LOCKED", 0),
            Err(LayerError::InvalidColorIndex(0))
        );
        assert!(!editor.is_dirty());
        assert!(!editor.can_undo());
        editor.set_layer_color("LOCKED", 255).unwrap();
        assert_eq!(editor.selection(), [0, 3]);
        assert!(editor.layer_for_entity(3).unwrap().is_locked());
        assert!(editor.is_dirty());
        editor.undo();
        assert_eq!(editor.to_dxf(), source);
        assert!(!editor.is_dirty());
        editor.redo();
        editor.mark_saved();
        let saved = editor.to_dxf();
        editor.set_layer_color("HIDDEN", 11).unwrap();
        assert_eq!(editor.layer_for_entity(1).unwrap().color_index(), -11);
        assert!(!editor.is_entity_visible(1));
        editor.undo();
        assert_eq!(editor.to_dxf(), saved);
        assert_eq!(editor.selection(), [0, 3]);
        assert!(!editor.is_dirty());
    }

    #[test]
    fn layer_history_filters_hidden_selection_and_protects_locked_geometry() {
        let source = include_str!("../tests/fixtures/layers.dxf");
        let mut editor = Editor::new(DxfDocument::parse(source).unwrap());
        editor.select(vec![0, 1, 2, 3, 4]);
        assert_eq!(editor.selection(), [0, 3, 4]);
        assert!(matches!(
            editor.translate_selection(1.0, 2.0, 3.0, false),
            Err(EditError::Layer(LayerError::LockedLayer(_)))
        ));
        assert!(matches!(
            editor.delete_selection(),
            Err(LayerError::LockedLayer(_))
        ));
        assert_eq!(editor.to_dxf(), source);
        assert!(!editor.can_undo());
        assert!(!editor.is_dirty());
        editor.add_layer("Notes").unwrap();
        editor.set_current_layer("notes").unwrap();
        editor.mark_saved();
        editor.set_current_layer("walls").unwrap();
        assert!(!editor.is_dirty());
        editor.select(vec![0, 4]);
        editor.translate_selection(10.0, 20.0, 30.0, true).unwrap();
        assert_eq!(editor.selection(), [5, 6]);
        assert_eq!(editor.entity_layer(5), Some("WALLS"));
        assert_eq!(editor.entity_layer(6), Some("0"));
        editor.assign_selection_layer("Notes").unwrap();
        assert_eq!(editor.entity_layer(5), Some("Notes"));
        assert_eq!(editor.entity_layer(6), Some("Notes"));
        editor.undo();
        assert_eq!(editor.entity_layer(5), Some("WALLS"));
        assert_eq!(editor.entity_layer(6), Some("0"));
        editor.redo();
        editor.mark_saved();
        let saved = editor.to_dxf();
        editor.set_layer_visible("Notes", false).unwrap();
        assert!(editor.selection().is_empty());
        assert_eq!(editor.visible_entities().len(), 3);
        editor.undo();
        assert_eq!(editor.to_dxf(), saved);
        assert_eq!(editor.selection(), [5, 6]);
        assert!(!editor.is_dirty());
        editor.set_layer_locked("Notes", true).unwrap();
        let locked = editor.to_dxf();
        editor.set_current_layer("Notes").unwrap();
        assert!(matches!(
            editor.add(vec![editor.entities()[0].clone()]),
            Err(LayerError::LockedLayer(_))
        ));
        let replacement = editor.entities()[5].translated(1.0, 0.0, 0.0).unwrap();
        assert!(matches!(
            editor.update(5, replacement),
            Err(LayerError::LockedLayer(_))
        ));
        assert_eq!(editor.to_dxf(), locked);
        editor.set_layer_locked("Notes", false).unwrap();
        editor.delete_selection().unwrap();
        assert_eq!(editor.entities().len(), 5);
        editor.undo();
        assert_eq!(editor.entities().len(), 7);
        let reopened = DxfDocument::parse(&editor.to_dxf()).unwrap();
        assert_eq!(reopened.entity_layer(5), Some("Notes"));
    }

    #[test]
    fn drawing_transactions_restore_source_records_selection_and_savepoint() {
        let source = include_str!("../tests/fixtures/autocad-r12-lines.dxf");
        let mut editor = Editor::new(DxfDocument::parse(source).unwrap());
        editor.select(vec![0]);
        editor.translate_selection(1.0, -2.0, 3.0, false).unwrap();
        assert!(editor.is_dirty());
        let Entity::Line(line) = editor.entities()[0] else {
            panic!("expected LINE");
        };
        assert_eq!(line.start(), Point3::new(2.0, 0.0, 6.0).unwrap());
        editor.mark_saved();
        let saved = editor.to_dxf();
        editor.delete_selection().unwrap();
        assert_eq!(editor.entities().len(), 1);
        assert!(editor.is_dirty());
        editor.undo();
        assert_eq!(editor.to_dxf(), saved);
        assert_eq!(editor.selection(), [0]);
        assert!(!editor.is_dirty());
        editor.undo();
        assert_eq!(editor.to_dxf(), source);
        assert!(editor.is_dirty());
        editor.redo();
        assert!(!editor.is_dirty());
        editor
            .add(vec![Entity::Circle(
                Circle3::new(Point3::new(8.0, 9.0, 2.0).unwrap(), 4.0).unwrap(),
            )])
            .unwrap();
        assert!(!editor.can_redo());
        let imported = DxfDocument::parse(&editor.to_dxf()).unwrap();
        assert_eq!(imported.entities(), editor.entities());
        assert!(editor.to_dxf().contains("$ACADVER"));
        assert!(editor.to_dxf().contains("DIMSTYLE"));
        editor.undo();
        assert_eq!(editor.to_dxf(), saved);
    }

    #[test]
    fn failed_multi_entity_move_has_no_partial_changes_or_undo_entry() {
        let origin = Point3::new(0.0, 0.0, 0.0).unwrap();
        let large = Point3::new(f64::MAX, 0.0, 0.0).unwrap();
        let mut document = DxfDocument::default();
        document.push(Entity::Line(Line3::new(origin, origin)));
        document.push(Entity::Line(Line3::new(large, large)));
        let mut editor = Editor::new(document);
        editor.select(vec![0, 1]);
        let source = editor.to_dxf();
        assert!(
            editor
                .translate_selection(f64::MAX, 0.0, 0.0, false)
                .is_err()
        );
        assert_eq!(editor.to_dxf(), source);
        assert!(!editor.can_undo());
        assert!(!editor.is_dirty());
    }
}
