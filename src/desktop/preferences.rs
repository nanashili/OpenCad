use super::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const STORAGE_KEY: &str = "opencad.workspace.v1";

/// Owned, versioned application preferences. Drawing data, pending commands,
/// selection and unsaved edits deliberately have no representation here.
#[derive(Default, Deserialize, Serialize)]
#[serde(default)]
struct Preferences {
    panels: BTreeMap<String, bool>,
    slim_tools: bool,
    hidden_status: BTreeSet<String>,
    collapsed_toolsets: BTreeSet<String>,
    toolset_order: Vec<String>,
    // Group label -> first command ID -> visibility. Command IDs survive
    // catalog insertion/reordering, unlike the runtime item indices.
    tool_visibility: BTreeMap<String, BTreeMap<String, bool>>,
    recent_files: Vec<StoredPath>,
    recent_list: bool,
}

/// Preserve native filenames that cannot be represented by UTF-8. A profile
/// copied to another OS can still load its UI settings and skip foreign paths.
#[derive(Deserialize, Serialize)]
enum StoredPath {
    Text(String),
    Unix(Vec<u8>),
    Windows(Vec<u16>),
}

impl StoredPath {
    fn from_path(path: &Path) -> Option<Self> {
        if let Some(text) = path.to_str() {
            return Some(Self::Text(text.to_owned()));
        }
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            Some(Self::Unix(path.as_os_str().as_bytes().to_owned()))
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            Some(Self::Windows(path.as_os_str().encode_wide().collect()))
        }
        #[cfg(not(any(unix, windows)))]
        None
    }

    fn into_path(self) -> Option<PathBuf> {
        match self {
            Self::Text(text) => Some(PathBuf::from(text)),
            #[cfg(unix)]
            Self::Unix(bytes) => {
                use std::os::unix::ffi::OsStringExt;
                Some(std::ffi::OsString::from_vec(bytes).into())
            }
            #[cfg(windows)]
            Self::Windows(units) => {
                use std::os::windows::ffi::OsStringExt;
                Some(std::ffi::OsString::from_wide(&units).into())
            }
            _ => None,
        }
    }
}

impl shell::Workspace {
    fn preference_panels(&mut self) -> [(&'static str, &mut bool); 11] {
        [
            ("toolbar", &mut self.toolbar),
            ("file_tabs", &mut self.file_tabs),
            ("tools", &mut self.tools),
            ("layers", &mut self.layers),
            ("properties", &mut self.properties),
            ("command", &mut self.command),
            ("history", &mut self.history),
            ("status", &mut self.status),
            ("view_cube", &mut self.view_cube),
            ("ucs_icon", &mut self.ucs_icon),
            ("connect", &mut self.connect),
        ]
    }
}

impl OpenCadApp {
    pub(super) fn restore_preferences(&mut self, storage: &dyn eframe::Storage) {
        let Some(preferences) = eframe::get_value::<Preferences>(storage, STORAGE_KEY) else {
            if storage.get_string(STORAGE_KEY).is_some() {
                self.message(
                    "Workspace preferences could not be read; defaults are in use.",
                    true,
                );
            }
            return;
        };
        for (name, visible) in self.workspace.preference_panels() {
            if let Some(value) = preferences.panels.get(name) {
                *visible = *value;
            }
        }
        self.workspace.slim_tools = preferences.slim_tools;
        for item in catalog::STATUS_ITEMS {
            if preferences.hidden_status.contains(item.id) {
                self.workspace.hidden_status.insert(item.id);
            }
        }
        for group in catalog::TOOL_GROUPS {
            if preferences.collapsed_toolsets.contains(group.label) {
                self.workspace.collapsed_tools.insert(group.label);
            }
            if let Some(visibility) = preferences.tool_visibility.get(group.label) {
                for (index, item) in group.items.iter().enumerate() {
                    if let Some(visible) = item.actions.first().and_then(|id| visibility.get(*id))
                        && *visible != item.visible
                    {
                        self.workspace
                            .tool_visibility
                            .insert((group.label, index), *visible);
                    }
                }
            }
        }
        let mut order = Vec::new();
        for label in &preferences.toolset_order {
            if let Some(group) = catalog::TOOL_GROUPS
                .iter()
                .find(|group| group.label == label)
                && !order
                    .iter()
                    .any(|known: &&shell::ToolGroup| known.label == group.label)
            {
                order.push(group);
            }
        }
        for group in catalog::TOOL_GROUPS {
            if !order.iter().any(|known| known.label == group.label) {
                order.push(group);
            }
        }
        self.workspace.tool_order = order;
        self.recent_list = preferences.recent_list;
        for path in preferences
            .recent_files
            .into_iter()
            .filter_map(StoredPath::into_path)
        {
            if path.is_absolute() && !self.recent_files.contains(&path) {
                self.recent_files.push(path);
                if self.recent_files.len() == 20 {
                    break;
                }
            }
        }
    }

    pub(super) fn save_preferences(&mut self, storage: &mut dyn eframe::Storage) {
        let panels = self
            .workspace
            .preference_panels()
            .into_iter()
            .map(|(name, visible)| (name.to_owned(), *visible))
            .collect();
        let mut tool_visibility: BTreeMap<String, BTreeMap<String, bool>> = BTreeMap::new();
        for group in catalog::TOOL_GROUPS {
            for (index, item) in group.items.iter().enumerate() {
                if let Some(visible) = self.workspace.tool_visibility.get(&(group.label, index))
                    && let Some(id) = item.actions.first()
                {
                    tool_visibility
                        .entry(group.label.to_owned())
                        .or_default()
                        .insert((*id).to_owned(), *visible);
                }
            }
        }
        let preferences = Preferences {
            panels,
            slim_tools: self.workspace.slim_tools,
            hidden_status: self
                .workspace
                .hidden_status
                .iter()
                .map(|id| (*id).to_owned())
                .collect(),
            collapsed_toolsets: self
                .workspace
                .collapsed_tools
                .iter()
                .map(|label| (*label).to_owned())
                .collect(),
            toolset_order: self
                .workspace
                .tool_order
                .iter()
                .map(|group| group.label.to_owned())
                .collect(),
            tool_visibility,
            recent_files: self
                .recent_files
                .iter()
                .filter_map(|path| StoredPath::from_path(path))
                .collect(),
            recent_list: self.recent_list,
        };
        eframe::set_value(storage, STORAGE_KEY, &preferences);
    }
}
