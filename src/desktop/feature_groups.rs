use super::*;
use crate::DrawOrder;

impl OpenCadApp {
    pub(super) fn feature_groups_command(
        &mut self,
        verb: &str,
        tokens: &[&str],
    ) -> Option<Result<(), String>> {
        let result = match verb {
            "GROUP" => self.group_command(tokens),
            "UNGROUP" => self.ungroup_command(tokens),
            "DRAWORDER" => self.draw_order_command(tokens),
            _ => return None,
        };
        Some(result)
    }

    fn group_index(&self, name: &str) -> Option<usize> {
        self.editor
            .groups()
            .iter()
            .position(|group| group.name().eq_ignore_ascii_case(name))
    }

    fn group_command(&mut self, tokens: &[&str]) -> Result<(), String> {
        let action = tokens.first().copied().unwrap_or("LIST");
        match action.to_ascii_uppercase().as_str() {
            "CREATE" | "NEW" => {
                let name = tokens.get(1).ok_or("GROUP CREATE name [description].")?;
                let description = tokens.get(2).copied().unwrap_or_default();
                self.editor
                    .create_group_from_selection(*name, description, true)
                    .map_err(|error| error.to_string())?;
                self.message(format!("Group {name} created."), false);
            }
            "LIST" => {
                let names = self
                    .editor
                    .groups()
                    .iter()
                    .map(|group| group.name())
                    .collect::<Vec<_>>()
                    .join(", ");
                self.message(
                    if names.is_empty() {
                        "No groups defined.".into()
                    } else {
                        names
                    },
                    false,
                );
            }
            "SELECT" => {
                let name = tokens.get(1).ok_or("GROUP SELECT name.")?;
                let group = self
                    .editor
                    .groups()
                    .iter()
                    .find(|group| group.name().eq_ignore_ascii_case(name))
                    .ok_or_else(|| format!("Unknown group {name}."))?;
                if !group.selectable() {
                    return Err(format!("Group {name} is not selectable."));
                }
                self.editor.select(group.members().to_vec());
                self.message(format!("Group {name} selected."), false);
            }
            "ADD" | "REMOVE" => {
                let name = tokens.get(1).ok_or("GROUP ADD|REMOVE name.")?;
                let index = self
                    .group_index(name)
                    .ok_or_else(|| format!("Unknown group {name}."))?;
                let mut members = self.editor.groups()[index].members().to_vec();
                if action.eq_ignore_ascii_case("ADD") {
                    members.extend_from_slice(self.editor.selection());
                } else {
                    members.retain(|member| !self.editor.selection().contains(member));
                }
                self.editor
                    .set_group_membership(index, &members)
                    .map_err(|error| error.to_string())?;
                self.message(format!("Group {name} membership updated."), false);
            }
            "RENAME" => {
                let old = tokens.get(1).ok_or("GROUP RENAME old-name new-name.")?;
                let new = tokens.get(2).ok_or("GROUP RENAME old-name new-name.")?;
                let index = self
                    .group_index(old)
                    .ok_or_else(|| format!("Unknown group {old}."))?;
                self.editor
                    .rename_group(index, *new)
                    .map_err(|error| error.to_string())?;
                self.message(format!("Group renamed to {new}."), false);
            }
            "DELETE" | "REMOVEGROUP" => {
                let name = tokens.get(1).ok_or("GROUP DELETE name.")?;
                let index = self
                    .group_index(name)
                    .ok_or_else(|| format!("Unknown group {name}."))?;
                self.editor
                    .remove_group(index)
                    .map_err(|error| error.to_string())?;
                self.message(format!("Group {name} deleted."), false);
            }
            _ => return Err("GROUP CREATE|SELECT|ADD|REMOVE|RENAME|DELETE|LIST ...".into()),
        }
        Ok(())
    }

    fn ungroup_command(&mut self, tokens: &[&str]) -> Result<(), String> {
        let name = tokens.first().ok_or("UNGROUP name.")?;
        let index = self
            .group_index(name)
            .ok_or_else(|| format!("Unknown group {name}."))?;
        self.editor
            .remove_group(index)
            .map_err(|error| error.to_string())?;
        self.message(format!("Group {name} removed."), false);
        Ok(())
    }

    fn draw_order_command(&mut self, tokens: &[&str]) -> Result<(), String> {
        let action = tokens
            .first()
            .ok_or("DRAWORDER FRONT|BACK|ABOVE|BELOW [entity].")?;
        let selected = self.editor.selection().to_vec();
        if selected.is_empty() {
            return Err("Select entities before changing draw order.".into());
        }
        let order = match action.to_ascii_uppercase().as_str() {
            "FRONT" => DrawOrder::Front,
            "BACK" => DrawOrder::Back,
            "ABOVE" | "BELOW" => {
                let reference = tokens
                    .get(1)
                    .ok_or("DRAWORDER ABOVE|BELOW needs an entity number.")?
                    .parse::<usize>()
                    .ok()
                    .and_then(|number| number.checked_sub(1))
                    .ok_or("Reference entity must be a positive number.")?;
                if action.eq_ignore_ascii_case("ABOVE") {
                    DrawOrder::Above(reference)
                } else {
                    DrawOrder::Below(reference)
                }
            }
            _ => return Err("DRAWORDER FRONT|BACK|ABOVE|BELOW [entity].".into()),
        };
        let permutation =
            Editor::draw_order_permutation(self.editor.entities().len(), &selected, order)
                .map_err(|error| error.to_string())?;
        self.editor
            .reorder_entities_with(&permutation, |document, order| {
                document.reorder_entities(order)
            })
            .map_err(|error| error.to_string())?;
        self.message("Draw order updated.", false);
        Ok(())
    }
}
