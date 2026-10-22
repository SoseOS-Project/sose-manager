// utils: small helpers to make terminal output easier to read.
//
// colors only get used when we're printing to a real terminal and NO_COLOR
// isn't set, so logs and pipes (svm list > file) stay plain text.

use std::io::IsTerminal;

use owo_colors::OwoColorize;

// gap between table columns
const GAP: usize = 4;

// the looks i use everywhere
#[derive(Clone, Copy)]
pub enum Style {
    Plain,
    On,     // running vm, green
    Off,    // powered off vm, dim red
    Ok,     // dim green
    Warn,   // yellow
    Dim,    // grey-ish, for less important stuff
    Header, // bold, column names
}

// should we print colors at all
pub fn color_enabled() -> bool {
    std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

// text in one of the styles above (or untouched if colors are off)
pub fn paint(text: &str, style: Style) -> String {
    if !color_enabled() {
        return text.to_string();
    }

    match style {
        Style::Plain => text.to_string(),
        Style::On => text.green().to_string(),
        Style::Off => text.red().dimmed().to_string(),
        Style::Ok => text.green().dimmed().to_string(),
        Style::Warn => text.yellow().to_string(),
        Style::Dim => text.dimmed().to_string(),
        Style::Header => text.bold().to_string(),
    }
}

// one table cell: its text and how to color it
pub struct Cell {
    pub text: String,
    pub style: Style,
}

impl Cell {
    pub fn new(text: &str, style: Style) -> Cell {
        Cell {
            text: text.to_string(),
            style,
        }
    }
}

// lines rows up into columns with the column names on top.
// pads with spaces instead of \t, tabs don't line up once a name is longer than 8 chars.
pub fn table(headers: &[&str], rows: &[Vec<Cell>]) -> String {
    // widest thing in each column, header included
    let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i < widths.len() && cell.text.len() > widths[i] {
                widths[i] = cell.text.len();
            }
        }
    }

    let mut out = String::new();

    // padding has to happen before painting, the color codes would mess up the width
    for (i, header) in headers.iter().enumerate() {
        let padded = format!("{:<w$}", header, w = widths[i] + GAP);
        out.push_str(&paint(&padded, Style::Header));
    }
    out.push('\n');

    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            let width = widths.get(i).copied().unwrap_or(0) + GAP;
            let padded = format!("{:<w$}", cell.text, w = width);
            out.push_str(&paint(&padded, cell.style));
        }
        out.push('\n');
    }

    out
}
