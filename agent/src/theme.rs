//! Theme system: named color schemes for the TUI.

use ratatui::style::Color;

#[derive(Debug, Clone)]
pub struct Theme {
    pub name: &'static str,
    pub user_header: Color,
    pub assistant_header: Color,
    pub tool: Color,
    pub tool_result: Color,
    pub error: Color,
    pub status: Color,
    pub system: Color,
    pub separator: Color,
    pub input_text: Color,
    pub input_border: Color,
    pub status_bar_fg: Color,
    pub status_bar_bg: Color,
}

pub const THEMES: &[Theme] = &[
    Theme {
        name: "default",
        user_header: Color::Cyan,
        assistant_header: Color::Green,
        tool: Color::Yellow,
        tool_result: Color::DarkGray,
        error: Color::Red,
        status: Color::Yellow,
        system: Color::Blue,
        separator: Color::DarkGray,
        input_text: Color::White,
        input_border: Color::DarkGray,
        status_bar_fg: Color::DarkGray,
        status_bar_bg: Color::Black,
    },
    Theme {
        name: "dracula",
        user_header: Color::Rgb(139, 233, 253),  // cyan
        assistant_header: Color::Rgb(80, 250, 123),  // green
        tool: Color::Rgb(241, 250, 140),  // yellow
        tool_result: Color::Rgb(98, 114, 164),  // comment
        error: Color::Rgb(255, 85, 85),  // red
        status: Color::Rgb(255, 184, 108),  // orange
        system: Color::Rgb(189, 147, 249),  // purple
        separator: Color::Rgb(68, 71, 90),  // current line
        input_text: Color::Rgb(248, 248, 242),  // foreground
        input_border: Color::Rgb(68, 71, 90),
        status_bar_fg: Color::Rgb(248, 248, 242),
        status_bar_bg: Color::Rgb(40, 42, 54),  // background
    },
    Theme {
        name: "nord",
        user_header: Color::Rgb(136, 192, 208),  // frost
        assistant_header: Color::Rgb(163, 190, 140),  // green
        tool: Color::Rgb(235, 203, 139),  // yellow
        tool_result: Color::Rgb(76, 86, 106),  // comment
        error: Color::Rgb(191, 97, 106),  // red
        status: Color::Rgb(208, 135, 112),  // orange
        system: Color::Rgb(180, 142, 173),  // purple
        separator: Color::Rgb(59, 66, 82),  // dark
        input_text: Color::Rgb(216, 222, 233),  // snow
        input_border: Color::Rgb(59, 66, 82),
        status_bar_fg: Color::Rgb(216, 222, 233),
        status_bar_bg: Color::Rgb(46, 52, 64),  // polar night
    },
    Theme {
        name: "gruvbox",
        user_header: Color::Rgb(131, 165, 152),  // aqua
        assistant_header: Color::Rgb(184, 187, 38),  // green
        tool: Color::Rgb(250, 189, 47),  // yellow
        tool_result: Color::Rgb(146, 131, 116),  // gray
        error: Color::Rgb(251, 73, 52),  // red
        status: Color::Rgb(254, 128, 25),  // orange
        system: Color::Rgb(211, 134, 155),  // purple
        separator: Color::Rgb(80, 73, 69),  // dark
        input_text: Color::Rgb(235, 219, 178),  // fg
        input_border: Color::Rgb(80, 73, 69),
        status_bar_fg: Color::Rgb(235, 219, 178),
        status_bar_bg: Color::Rgb(40, 40, 40),  // bg
    },
    Theme {
        name: "tokyonight",
        user_header: Color::Rgb(125, 207, 255),  // cyan
        assistant_header: Color::Rgb(158, 206, 106),  // green
        tool: Color::Rgb(224, 175, 104),  // yellow
        tool_result: Color::Rgb(86, 95, 137),  // comment
        error: Color::Rgb(219, 75, 75),  // red
        status: Color::Rgb(255, 158, 100),  // orange
        system: Color::Rgb(187, 154, 247),  // purple
        separator: Color::Rgb(41, 46, 66),  // dark
        input_text: Color::Rgb(192, 202, 245),  // fg
        input_border: Color::Rgb(41, 46, 66),
        status_bar_fg: Color::Rgb(192, 202, 245),
        status_bar_bg: Color::Rgb(26, 27, 38),  // bg
    },
    Theme {
        name: "catppuccin",
        user_header: Color::Rgb(137, 220, 235),  // sapphire
        assistant_header: Color::Rgb(166, 218, 149),  // green
        tool: Color::Rgb(238, 212, 159),  // yellow
        tool_result: Color::Rgb(110, 115, 141),  // overlay0
        error: Color::Rgb(237, 135, 150),  // red
        status: Color::Rgb(245, 169, 127),  // peach
        system: Color::Rgb(198, 160, 246),  // mauve
        separator: Color::Rgb(54, 58, 79),  // surface0
        input_text: Color::Rgb(202, 211, 245),  // text
        input_border: Color::Rgb(54, 58, 79),
        status_bar_fg: Color::Rgb(202, 211, 245),
        status_bar_bg: Color::Rgb(30, 30, 46),  // base
    },
    Theme {
        name: "rosepine",
        user_header: Color::Rgb(156, 207, 216),  // foam
        assistant_header: Color::Rgb(49, 116, 143),  // pine
        tool: Color::Rgb(246, 193, 119),  // gold
        tool_result: Color::Rgb(110, 106, 134),  // muted
        error: Color::Rgb(235, 111, 146),  // love
        status: Color::Rgb(234, 154, 151),  // rose
        system: Color::Rgb(196, 167, 231),  // iris
        separator: Color::Rgb(38, 35, 53),  // overlay
        input_text: Color::Rgb(224, 222, 244),  // text
        input_border: Color::Rgb(38, 35, 53),
        status_bar_fg: Color::Rgb(224, 222, 244),
        status_bar_bg: Color::Rgb(25, 23, 36),  // base
    },
];

pub fn get(name: &str) -> &'static Theme {
    THEMES
        .iter()
        .find(|t| t.name == name)
        .unwrap_or(&THEMES[0])
}

pub fn list_names() -> Vec<&'static str> {
    THEMES.iter().map(|t| t.name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_default_theme() {
        let t = get("default");
        assert_eq!(t.name, "default");
    }

    #[test]
    fn test_get_unknown_returns_default() {
        let t = get("nonexistent");
        assert_eq!(t.name, "default");
    }

    #[test]
    fn test_list_names() {
        let names = list_names();
        assert!(names.contains(&"default"));
        assert!(names.contains(&"dracula"));
        assert!(names.contains(&"nord"));
        assert!(names.len() >= 7);
    }
}
