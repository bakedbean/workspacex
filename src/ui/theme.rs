use ratatui::style::{Color, Style};

pub fn header() -> Style { Style::default().fg(Color::Cyan).bg(Color::Reset) }
pub fn selected() -> Style { Style::default().fg(Color::Black).bg(Color::Cyan) }
pub fn dim() -> Style { Style::default().fg(Color::DarkGray) }
pub fn ok() -> Style { Style::default().fg(Color::Green) }
pub fn warn() -> Style { Style::default().fg(Color::Yellow) }
pub fn err() -> Style { Style::default().fg(Color::Red) }
