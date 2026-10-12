//! Bounded, document-aware expansion of block references for derived geometry.

use crate::{DxfDocument, Entity};
use std::collections::HashSet;
use std::fmt;

const MAX_EXPANSION_DEPTH: usize = 32;
const MAX_EXPANDED_ENTITIES: usize = 100_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockExpansionError {
    UnresolvedBlock(String),
    RecursiveBlock(String),
    DepthLimit,
    EntityLimit,
    Dynamic(String),
    Geometry(String),
}

impl fmt::Display for BlockExpansionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnresolvedBlock(name) => write!(formatter, "unresolved block {name}"),
            Self::RecursiveBlock(name) => write!(formatter, "recursive block reference {name}"),
            Self::DepthLimit => write!(
                formatter,
                "block nesting exceeds {MAX_EXPANSION_DEPTH} levels"
            ),
            Self::EntityLimit => write!(
                formatter,
                "block expansion exceeds {MAX_EXPANDED_ENTITIES} entities"
            ),
            Self::Dynamic(message) => write!(formatter, "invalid dynamic block data: {message}"),
            Self::Geometry(message) => write!(formatter, "block geometry error: {message}"),
        }
    }
}

impl std::error::Error for BlockExpansionError {}

/// One flattened entity and the top-level drawing entity that owns it.
#[derive(Clone, Debug, PartialEq)]
pub struct ExpandedEntity {
    pub owner_index: usize,
    pub entity: Entity,
}

/// Expand one INSERT or MLEADER block payload into drawable leaf geometry.
/// Non-reference entities are returned unchanged.
pub fn expand_entity(
    document: &DxfDocument,
    entity: &Entity,
) -> Result<Vec<Entity>, BlockExpansionError> {
    let mut output = Vec::new();
    let mut context = ExpansionContext::new(document);
    context.expand(entity, 0, &mut output)?;
    Ok(output)
}

/// Expand every document entity while retaining its top-level selection owner.
pub fn expand_entities(document: &DxfDocument) -> Result<Vec<ExpandedEntity>, BlockExpansionError> {
    let mut output = Vec::new();
    for (owner_index, entity) in document.entities().iter().enumerate() {
        let mut expanded = Vec::new();
        let mut context = ExpansionContext::new(document);
        context.expand(entity, 0, &mut expanded)?;
        if output.len().saturating_add(expanded.len()) > MAX_EXPANDED_ENTITIES {
            return Err(BlockExpansionError::EntityLimit);
        }
        output.extend(expanded.into_iter().map(|entity| ExpandedEntity {
            owner_index,
            entity,
        }));
    }
    Ok(output)
}

struct ExpansionContext<'a> {
    document: &'a DxfDocument,
    block_stack: HashSet<String>,
}

impl<'a> ExpansionContext<'a> {
    fn new(document: &'a DxfDocument) -> Self {
        Self {
            document,
            block_stack: HashSet::new(),
        }
    }

    fn expand(
        &mut self,
        entity: &Entity,
        depth: usize,
        output: &mut Vec<Entity>,
    ) -> Result<(), BlockExpansionError> {
        if depth > MAX_EXPANSION_DEPTH {
            return Err(BlockExpansionError::DepthLimit);
        }
        match entity {
            Entity::Insert(insert) => self.expand_insert(insert, depth, output),
            Entity::MLeader(leader) => {
                push_entity(output, entity.clone())?;
                if let crate::MLeaderContent::Block(insert) = leader.content() {
                    self.expand(&Entity::Insert(insert.clone()), depth + 1, output)?;
                }
                Ok(())
            }
            _ => push_entity(output, entity.clone()),
        }
    }

    fn expand_insert(
        &mut self,
        insert: &crate::Insert,
        depth: usize,
        output: &mut Vec<Entity>,
    ) -> Result<(), BlockExpansionError> {
        let name = insert.block_name();
        let key = name.to_ascii_uppercase();
        if !self.block_stack.insert(key.clone()) {
            return Err(BlockExpansionError::RecursiveBlock(name.into()));
        }
        let result = self.expand_definition(insert, depth, output);
        self.block_stack.remove(&key);
        result
    }

    fn expand_definition(
        &mut self,
        insert: &crate::Insert,
        depth: usize,
        output: &mut Vec<Entity>,
    ) -> Result<(), BlockExpansionError> {
        let name = insert.block_name();
        let definition = self
            .document
            .block(name)
            .ok_or_else(|| BlockExpansionError::UnresolvedBlock(name.into()))?;
        let children = evaluate_insert_geometry(definition, insert)?;
        if output.len().saturating_add(children.len()) > MAX_EXPANDED_ENTITIES {
            return Err(BlockExpansionError::EntityLimit);
        }
        for child in children {
            self.expand(&child, depth + 1, output)?;
        }
        Ok(())
    }
}

fn evaluate_insert_geometry(
    definition: &crate::BlockDefinition,
    insert: &crate::Insert,
) -> Result<Vec<Entity>, BlockExpansionError> {
    let evaluated_definition = if let Some(dynamic) = definition.dynamic_definition() {
        let local = crate::dynamic_block::evaluate_local_entities(
            definition,
            dynamic,
            insert.dynamic_overrides(),
        )
        .map_err(|error| BlockExpansionError::Dynamic(error.to_string()))?;
        ensure_static_output_bound(local.len(), insert)?;
        let mut evaluated = definition.clone();
        evaluated.set_entities(local);
        evaluated
    } else {
        ensure_static_output_bound(definition.entities().len(), insert)?;
        definition.clone()
    };
    insert
        .explode(&evaluated_definition)
        .map_err(|error| BlockExpansionError::Geometry(format!("{error:?}")))
}

fn ensure_static_output_bound(
    entity_count: usize,
    insert: &crate::Insert,
) -> Result<(), BlockExpansionError> {
    let rows = insert.rows().unwrap_or(1);
    let columns = insert.columns().unwrap_or(1);
    let count = entity_count
        .checked_mul(rows)
        .and_then(|count| count.checked_mul(columns))
        .ok_or(BlockExpansionError::EntityLimit)?;
    if count > MAX_EXPANDED_ENTITIES {
        return Err(BlockExpansionError::EntityLimit);
    }
    Ok(())
}

fn push_entity(output: &mut Vec<Entity>, entity: Entity) -> Result<(), BlockExpansionError> {
    check_output_bound(output, 1)?;
    output.push(entity);
    Ok(())
}

fn check_output_bound(output: &[Entity], additional: usize) -> Result<(), BlockExpansionError> {
    if output.len().saturating_add(additional) > MAX_EXPANDED_ENTITIES {
        return Err(BlockExpansionError::EntityLimit);
    }
    Ok(())
}
