use colored::Color;

/// Color theme for CLI output.
#[allow(dead_code)]
pub struct Theme {
    pub success: Color,
    pub error: Color,
    pub warning: Color,
    pub info: Color,
    pub header: Color,
    pub muted: Color,
    pub accent: Color,
    pub branch: Color,
    pub hash: Color,
    pub added: Color,
    pub removed: Color,
    pub modified: Color,
}

#[allow(dead_code)]
impl Theme {
    /// Returns the default Worktree color theme.
    pub fn default_theme() -> Self {
        Self {
            success: Color::Green,
            error: Color::Red,
            warning: Color::Yellow,
            info: Color::Cyan,
            header: Color::BrightWhite,
            muted: Color::BrightBlack,
            accent: Color::Magenta,
            branch: Color::BrightCyan,
            hash: Color::Yellow,
            added: Color::Green,
            removed: Color::Red,
            modified: Color::Yellow,
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::default_theme()
    }
}

// Convenience constants using the default theme colors.
#[allow(dead_code)]
pub const SUCCESS_COLOR: Color = Color::Green;
#[allow(dead_code)]
pub const ERROR_COLOR: Color = Color::Red;
#[allow(dead_code)]
pub const WARNING_COLOR: Color = Color::Yellow;
#[allow(dead_code)]
pub const INFO_COLOR: Color = Color::Cyan;
#[allow(dead_code)]
pub const HEADER_COLOR: Color = Color::BrightWhite;
#[allow(dead_code)]
pub const MUTED_COLOR: Color = Color::BrightBlack;
#[allow(dead_code)]
pub const ACCENT_COLOR: Color = Color::Magenta;
#[allow(dead_code)]
pub const BRANCH_COLOR: Color = Color::BrightCyan;
#[allow(dead_code)]
pub const HASH_COLOR: Color = Color::Yellow;
#[allow(dead_code)]
pub const ADDED_COLOR: Color = Color::Green;
#[allow(dead_code)]
pub const REMOVED_COLOR: Color = Color::Red;
#[allow(dead_code)]
pub const MODIFIED_COLOR: Color = Color::Yellow;
