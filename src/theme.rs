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

/// Detect terminal background color via OSC 11 query.
/// Returns true for dark, false for light. Defaults to dark on failure.
pub fn detect_dark_background() -> bool {
    use std::io::{Read, Write};

    // Send OSC 11 query: request background color
    let mut stdout = std::io::stdout();
    if write!(stdout, "\x1b]11;?\x07").is_err() || stdout.flush().is_err() {
        return true; // default dark
    }

    // Try to read the response with a short timeout
    // Response format: \x1b]11;rgb:RRRR/GGGG/BBBB\x07
    let mut stdin = std::io::stdin();

    // Set a very short deadline — we can't block the TUI startup
    // Use a non-blocking approach: put stdin in raw mode temporarily
    // and poll for data. If no response in 100ms, assume dark.
    let mut buf = [0u8; 64];
    let mut collected = Vec::new();

    // We're already in raw mode from crossterm when this is called,
    // so stdin should be non-blocking enough with a poll
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(100);
    while std::time::Instant::now() < deadline {
        if crossterm::event::poll(std::time::Duration::from_millis(10)).unwrap_or(false) {
            if let Ok(n) = stdin.read(&mut buf) {
                collected.extend_from_slice(&buf[..n]);
                let s = String::from_utf8_lossy(&collected);
                if s.contains("\x07") || s.contains("\x1b\\") {
                    break;
                }
            }
        }
    }

    let response = String::from_utf8_lossy(&collected);
    parse_osc11_luminance(&response)
        .map(|lum| lum < 0.5) // dark if luminance < 50%
        .unwrap_or(true) // default dark
}

/// Parse luminance from an OSC 11 response.
/// Format: \x1b]11;rgb:RRRR/GGGG/BBBB\x07
fn parse_osc11_luminance(response: &str) -> Option<f64> {
    let rgb_start = response.find("rgb:")?;
    let rgb = &response[rgb_start + 4..];
    let parts: Vec<&str> = rgb.split(|c| c == '/' || c == '\x07' || c == '\x1b')
        .take(3)
        .collect();
    if parts.len() < 3 {
        return None;
    }
    // Values are hex, 2 or 4 digits each
    let r = u32::from_str_radix(parts[0], 16).ok()? as f64;
    let g = u32::from_str_radix(parts[1], 16).ok()? as f64;
    let b = u32::from_str_radix(parts[2], 16).ok()? as f64;
    // Normalize: if 4-digit hex, max is 0xFFFF; if 2-digit, max is 0xFF
    let max_val = if parts[0].len() > 2 { 65535.0 } else { 255.0 };
    // Relative luminance (ITU-R BT.709)
    Some(0.2126 * (r / max_val) + 0.7152 * (g / max_val) + 0.0722 * (b / max_val))
}

/// Pick a default theme based on terminal background.
pub fn auto_theme() -> &'static str {
    if detect_dark_background() {
        "default" // already dark-friendly
    } else {
        "default" // TODO: add a light theme variant
    }
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

    #[test]
    fn test_parse_osc11_dark() {
        // Dark background: rgb:0000/0000/0000
        let lum = parse_osc11_luminance("\x1b]11;rgb:0000/0000/0000\x07");
        assert!(lum.is_some());
        assert!(lum.unwrap() < 0.5);
    }

    #[test]
    fn test_parse_osc11_light() {
        // Light background: rgb:FFFF/FFFF/FFFF
        let lum = parse_osc11_luminance("\x1b]11;rgb:FFFF/FFFF/FFFF\x07");
        assert!(lum.is_some());
        assert!(lum.unwrap() > 0.5);
    }

    #[test]
    fn test_parse_osc11_invalid() {
        assert!(parse_osc11_luminance("garbage").is_none());
    }
}
