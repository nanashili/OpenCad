use crate::{
    BoundaryMode, Corner, CornerParameter, DxfDocument, Entity, EntityStyle, EntityStyleError,
    GeometryError, Layer, LayerError, Lengthen, Linetype, LinetypeError, Offset, OffsetLayer,
    OffsetOptions, Transform,
};
use std::{error::Error, fmt};

#[derive(Debug)]
pub enum EditError {
    Geometry(GeometryError),
    Layer(LayerError),
    Style(EntityStyleError),
}
impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Geometry(error) => error.fmt(f),
            Self::Layer(error) => error.fmt(f),
            Self::Style(error) => error.fmt(f),
        }
    }
}
impl Error for EditError {}
impl From<GeometryError> for EditError {
    fn from(error: GeometryError) -> Self {
        Self::Geometry(error)
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

#[derive(Clone)]
struct Snapshot {
    document: DxfDocument,
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
}

impl Default for Editor {
    fn default() -> Self {
        Self::new(DxfDocument::default())
    }
}

impl Editor {
    pub fn new(document: DxfDocument) -> Self {
        Self {
            current: Snapshot {
                document,
                selection: Vec::new(),
                revision: 0,
            },
            undo: Vec::new(),
            redo: Vec::new(),
            next_revision: 1,
            saved_revision: 0,
            active_layer: "0".into(),
        }
    }

    pub fn entities(&self) -> &[Entity] {
        self.current.document.entities()
    }
    pub fn selection(&self) -> &[usize] {
        &self.current.selection
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
        self.layer_for_entity(index).is_some_and(Layer::is_visible)
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
        self.current.document = document;
        self.select(self.selection().to_vec());
        Ok(true)
    }
    pub fn add_layer(&mut self, name: &str) -> Result<bool, LayerError> {
        self.edit_document(|document| {
            document.add_layer(name)?;
            Ok(true)
        })
    }
    pub fn set_layer_visible(&mut self, name: &str, visible: bool) -> Result<bool, LayerError> {
        self.edit_document(|document| document.set_layer_visible(name, visible))
    }
    pub fn set_layer_locked(&mut self, name: &str, locked: bool) -> Result<bool, LayerError> {
        self.edit_document(|document| document.set_layer_locked(name, locked))
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

    pub fn add(&mut self, entities: Vec<Entity>) -> Result<bool, LayerError> {
        if entities.is_empty() {
            return Ok(false);
        }
        self.editable_layer(&self.active_layer)?;
        self.checkpoint();
        let first = self.entities().len();
        for entity in entities {
            self.current
                .document
                .push_on_layer(entity, &self.active_layer)
                .expect("validated current layer");
        }
        self.select((first..self.entities().len()).collect());
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
        self.checkpoint();
        self.current
            .document
            .replace_entity(index, entity)
            .expect("validated existing entity type");
        Ok(true)
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
                    )
                })
                .collect();
            self.checkpoint();
            let first = self.entities().len();
            for (entity, (layer, style)) in transformed.into_iter().zip(styles) {
                self.current
                    .document
                    .push_with_style(entity, &layer, style)
                    .expect("existing source layer");
            }
            self.select((first..self.entities().len()).collect());
            return Ok(true);
        }
        if transformed
            .iter()
            .zip(self.selection())
            .all(|(entity, index)| *entity == self.entities()[*index])
        {
            return Ok(false);
        }
        self.checkpoint();
        for (index, entity) in self.current.selection.iter().copied().zip(transformed) {
            self.current
                .document
                .replace_entity(index, entity)
                .expect("transform preserves type");
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
        self.checkpoint();
        if options.erase_source {
            self.current
                .document
                .remove(index)
                .expect("validated source index");
        }
        let created = self.entities().len();
        self.current
            .document
            .push_with_style(entity, &target_layer, style)
            .expect("validated target layer");
        self.select(vec![created]);
        Ok(true)
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
        self.checkpoint();
        if trim {
            self.current
                .document
                .replace_entity(first, Entity::Line(result.first()))
                .expect("validated source line");
            self.current
                .document
                .replace_entity(second, Entity::Line(result.second()))
                .expect("validated source line");
        }
        let mut selected = vec![first, second];
        if let Some(connector) = result.connector() {
            selected.push(self.entities().len());
            self.current
                .document
                .push_on_layer(connector.clone(), &connector_layer)
                .expect("validated connector layer");
        }
        self.select(selected);
        Ok(true)
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
        let same_type = parts
            .first()
            .is_some_and(|part| part.name() == source.name());
        self.checkpoint();
        let mut selected = Vec::new();
        if same_type {
            self.current
                .document
                .replace_entity(index, parts.remove(0))
                .expect("validated source type");
            selected.push(index);
        } else {
            self.current
                .document
                .remove(index)
                .expect("validated source index");
        }
        for part in parts {
            selected.push(self.entities().len());
            self.current
                .document
                .push_with_style(part, &layer, style.clone())
                .expect("validated source layer");
        }
        self.select(selected);
        Ok(true)
    }

    /// Replace selected polylines with their centerline components in one edit.
    /// Simple entities are ignored. All fallible work precedes the checkpoint;
    /// output records inherit source layer/style and become the new selection.
    pub fn explode_selection(&mut self) -> Result<usize, EditError> {
        let mut plans = Vec::new();
        for &index in self.selection() {
            let Entity::Polyline(polyline) = &self.entities()[index] else {
                continue;
            };
            let layer = self.entity_layer(index).expect("existing source layer");
            self.editable_layer(layer)?;
            let parts = polyline.explode()?;
            let style = self.entity_style(index).expect("existing source style");
            plans.push((index, layer.to_owned(), style.clone(), parts));
        }
        let count = plans.len();
        if count == 0 {
            return Ok(0);
        }
        self.checkpoint();
        for (index, ..) in plans.iter().rev() {
            self.current
                .document
                .remove(*index)
                .expect("validated source index");
        }
        let first = self.entities().len();
        for (_, layer, style, parts) in plans {
            for part in parts {
                self.current
                    .document
                    .push_with_style(part, &layer, style.clone())
                    .expect("validated source layer");
            }
        }
        self.select((first..self.entities().len()).collect());
        Ok(count)
    }

    pub fn delete_selection(&mut self) -> Result<bool, LayerError> {
        if self.selection().is_empty() {
            return Ok(false);
        }
        self.editable_selection()?;
        self.checkpoint();
        for index in self.current.selection.iter().rev().copied() {
            self.current
                .document
                .remove(index)
                .expect("selection contains existing entities");
        }
        self.current.selection.clear();
        Ok(true)
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
