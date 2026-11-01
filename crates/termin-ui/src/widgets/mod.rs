//! UI widgets and components (`termin-ui::widgets`).

pub mod pane;
pub mod scrollbar;
pub mod sidebar;
pub mod tab;
pub mod tabstrip;
pub mod workspace_card;

pub use pane::{PaneWidget, TAB_STRIP_HEIGHT};
pub use scrollbar::{Scrollbar, ScrollbarTick, ScrollbarTickKind};
pub use sidebar::{draw_sidebar, SidebarWorkspace};
pub use tab::{draw_tab, TabHits, TabState};
pub use tabstrip::{draw_tabstrip, StripHits};
pub use workspace_card::{draw_workspace_card, WorkspaceCardState};
