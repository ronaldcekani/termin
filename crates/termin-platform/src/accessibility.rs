//! Portable accessibility tree representations and VoiceOver contracts (`termin-platform::accessibility`).
//!
//! Provides the cross-platform accessibility data model used by assistive technologies
//! (such as macOS VoiceOver). Exposes workspace cards, tab strip tabs, split panes,
//! terminal output, command blocks, input editor, and announcements.
//!
//! See `docs/specs/platform-macos.md §10` and `docs/design/components.md §15`.

use termin_core::Rect;

/// Semantic accessibility role for an element in the Termin accessibility hierarchy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AccessibilityRole {
    /// Root application window.
    Window,
    /// Sidebar container.
    Sidebar,
    /// Workspace card inside the sidebar.
    WorkspaceCard,
    /// Tab strip containing per-pane tabs.
    TabStrip,
    /// Individual tab chip in a tab strip.
    Tab,
    /// Container holding split panes.
    SplitView,
    /// An individual terminal pane.
    Pane,
    /// Full screen / visible terminal text output (classic or block mode).
    TerminalOutput,
    /// Single execution block (prompt, command, output, exit status).
    CommandBlock,
    /// Input editor area at the bottom of a block-mode tab.
    InputEditor,
    /// Clickable action button (e.g. New Tab, Close, Split).
    Button,
    /// Static read-only text or status label.
    StaticText,
    /// Generic group container.
    Group,
}

impl AccessibilityRole {
    /// Return the native macOS NSAccessibility role string.
    pub fn macos_role_name(&self) -> &'static str {
        match self {
            Self::Window => "AXWindow",
            Self::Sidebar => "AXOutline",
            Self::WorkspaceCard => "AXRow",
            Self::TabStrip => "AXTabGroup",
            Self::Tab => "AXRadioButton",
            Self::SplitView => "AXSplitGroup",
            Self::Pane => "AXGroup",
            Self::TerminalOutput => "AXTextArea",
            Self::CommandBlock => "AXGroup",
            Self::InputEditor => "AXTextField",
            Self::Button => "AXButton",
            Self::StaticText => "AXStaticText",
            Self::Group => "AXGroup",
        }
    }
}

/// A node in the semantic accessibility element tree.
#[derive(Clone, Debug, PartialEq)]
pub struct AccessibilityNode {
    /// Unique identifier for this node within the snapshot.
    pub id: u64,
    /// Semantic role of the node.
    pub role: AccessibilityRole,
    /// Human-readable accessibility label read by screen readers.
    pub label: String,
    /// Current textual value or content (e.g. terminal text, editor input).
    pub value: Option<String>,
    /// Supplementary accessibility description / help text.
    pub description: Option<String>,
    /// Screen or window coordinates bounding this element.
    pub bounds: Rect,
    /// Whether this node currently has keyboard focus.
    pub focused: bool,
    /// Whether this node is selected (e.g. active workspace, active tab).
    pub selected: bool,
    /// Child accessibility nodes.
    pub children: Vec<AccessibilityNode>,
}

impl AccessibilityNode {
    /// Create a new accessibility node with an identifier, role, and label.
    pub fn new(id: u64, role: AccessibilityRole, label: impl Into<String>) -> Self {
        Self {
            id,
            role,
            label: label.into(),
            value: None,
            description: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            focused: false,
            selected: false,
            children: Vec::new(),
        }
    }

    /// Set value for text content.
    pub fn with_value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }

    /// Set supplementary description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Set bounding rectangle.
    pub fn with_bounds(mut self, bounds: Rect) -> Self {
        self.bounds = bounds;
        self
    }

    /// Set focused state.
    pub fn with_focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Set selected state.
    pub fn with_selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Set children.
    pub fn with_children(mut self, children: Vec<AccessibilityNode>) -> Self {
        self.children = children;
        self
    }

    /// Add a child node.
    pub fn add_child(&mut self, child: AccessibilityNode) {
        self.children.push(child);
    }

    /// Recursively search for a node by its ID.
    pub fn find_node(&self, id: u64) -> Option<&AccessibilityNode> {
        if self.id == id {
            return Some(self);
        }
        for child in &self.children {
            if let Some(found) = child.find_node(id) {
                return Some(found);
            }
        }
        None
    }

    /// Recursively find the currently focused node.
    pub fn find_focused(&self) -> Option<&AccessibilityNode> {
        if self.focused {
            return Some(self);
        }
        for child in &self.children {
            if let Some(found) = child.find_focused() {
                return Some(found);
            }
        }
        None
    }

    /// Find all nodes matching the given role in this subtree.
    pub fn find_by_role(&self, role: AccessibilityRole) -> Vec<&AccessibilityNode> {
        let mut result = Vec::new();
        self.collect_by_role(role, &mut result);
        result
    }

    fn collect_by_role<'a>(&'a self, role: AccessibilityRole, out: &mut Vec<&'a AccessibilityNode>) {
        if self.role == role {
            out.push(self);
        }
        for child in &self.children {
            child.collect_by_role(role, out);
        }
    }

    /// Generate a summary text line combining label, value, and description.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if !self.label.is_empty() {
            parts.push(self.label.as_str());
        }
        if let Some(ref val) = self.value {
            if !val.is_empty() {
                parts.push(val.as_str());
            }
        }
        if let Some(ref desc) = self.description {
            if !desc.is_empty() {
                parts.push(desc.as_str());
            }
        }
        parts.join(" - ")
    }
}

/// An immutable snapshot of the accessibility hierarchy for an application window.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AccessibilityTreeSnapshot {
    /// Root accessibility node (typically the Window or main container).
    pub root: Option<AccessibilityNode>,
    /// Explicit ID of the focused element.
    pub focused_id: Option<u64>,
}

impl AccessibilityTreeSnapshot {
    /// Create a new tree snapshot with a root node.
    pub fn new(root: AccessibilityNode) -> Self {
        let focused_id = root.find_focused().map(|n| n.id);
        Self {
            root: Some(root),
            focused_id,
        }
    }

    /// Create an empty snapshot.
    pub fn empty() -> Self {
        Self {
            root: None,
            focused_id: None,
        }
    }

    /// Get the root node reference.
    pub fn root(&self) -> Option<&AccessibilityNode> {
        self.root.as_ref()
    }

    /// Get the currently focused accessibility node.
    pub fn focused_node(&self) -> Option<&AccessibilityNode> {
        if let Some(id) = self.focused_id {
            if let Some(node) = self.find_node(id) {
                return Some(node);
            }
        }
        self.root.as_ref().and_then(|r| r.find_focused())
    }

    /// Search for a node by ID.
    pub fn find_node(&self, id: u64) -> Option<&AccessibilityNode> {
        self.root.as_ref().and_then(|r| r.find_node(id))
    }

    /// Find all nodes in the tree matching the given role.
    pub fn find_by_role(&self, role: AccessibilityRole) -> Vec<&AccessibilityNode> {
        self.root
            .as_ref()
            .map(|r| r.find_by_role(role))
            .unwrap_or_default()
    }

    /// Extract terminal screen text from the focused terminal output node.
    pub fn terminal_output_text(&self) -> Option<&str> {
        let outputs = self.find_by_role(AccessibilityRole::TerminalOutput);
        outputs
            .into_iter()
            .find(|n| n.focused)
            .or_else(|| self.find_by_role(AccessibilityRole::TerminalOutput).into_iter().next())
            .and_then(|n| n.value.as_deref())
    }

    /// Extract last command output text in block mode.
    pub fn last_command_output(&self) -> Option<&str> {
        let blocks = self.find_by_role(AccessibilityRole::CommandBlock);
        blocks.last().and_then(|n| n.value.as_deref())
    }

    /// List all workspace cards in the sidebar.
    pub fn workspace_cards(&self) -> Vec<&AccessibilityNode> {
        self.find_by_role(AccessibilityRole::WorkspaceCard)
    }

    /// List all tabs in the active tabstrip.
    pub fn tabs(&self) -> Vec<&AccessibilityNode> {
        self.find_by_role(AccessibilityRole::Tab)
    }
}

/// An accessibility announcement dispatched to assistive technologies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccessibilityAnnouncement {
    /// Spoken notification message for VoiceOver.
    pub message: String,
    /// High priority interruption flag (true interrupts current speech).
    pub high_priority: bool,
}

impl AccessibilityAnnouncement {
    /// Create a standard priority announcement.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            high_priority: false,
        }
    }

    /// Create a high-priority announcement (e.g. command failure or alert).
    pub fn high_priority(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            high_priority: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tree_construction_and_traversal() {
        let mut root = AccessibilityNode::new(1, AccessibilityRole::Window, "Termin");
        let mut sidebar = AccessibilityNode::new(2, AccessibilityRole::Sidebar, "Workspaces");
        let card1 = AccessibilityNode::new(3, AccessibilityRole::WorkspaceCard, "Dev")
            .with_selected(true)
            .with_description("Claude is waiting for input");
        let card2 = AccessibilityNode::new(4, AccessibilityRole::WorkspaceCard, "Docs")
            .with_selected(false);
        sidebar.add_child(card1);
        sidebar.add_child(card2);

        let mut pane = AccessibilityNode::new(5, AccessibilityRole::Pane, "Pane 1");
        let mut strip = AccessibilityNode::new(6, AccessibilityRole::TabStrip, "Tabs");
        let tab1 = AccessibilityNode::new(7, AccessibilityRole::Tab, "zsh")
            .with_selected(true);
        strip.add_child(tab1);

        let output = AccessibilityNode::new(8, AccessibilityRole::TerminalOutput, "Terminal")
            .with_value("hello world\n$ ls\nfoo bar")
            .with_focused(true);

        pane.add_child(strip);
        pane.add_child(output);

        root.add_child(sidebar);
        root.add_child(pane);

        let tree = AccessibilityTreeSnapshot::new(root);
        assert_eq!(tree.focused_id, Some(8));
        assert_eq!(tree.focused_node().map(|n| n.id), Some(8));
        assert_eq!(tree.terminal_output_text(), Some("hello world\n$ ls\nfoo bar"));

        let cards = tree.workspace_cards();
        assert_eq!(cards.len(), 2);
        assert_eq!(cards[0].label, "Dev");
        assert!(cards[0].selected);
        assert_eq!(cards[1].label, "Docs");

        let tabs = tree.tabs();
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs[0].label, "zsh");

        assert_eq!(AccessibilityRole::TerminalOutput.macos_role_name(), "AXTextArea");
        assert_eq!(AccessibilityRole::Tab.macos_role_name(), "AXRadioButton");
    }

    #[test]
    fn test_announcement_creation() {
        let a = AccessibilityAnnouncement::new("Command finished with exit code 0");
        assert!(!a.high_priority);
        assert_eq!(a.message, "Command finished with exit code 0");

        let a_high = AccessibilityAnnouncement::high_priority("Command failed");
        assert!(a_high.high_priority);
    }
}
