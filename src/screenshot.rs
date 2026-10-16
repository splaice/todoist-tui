//! Public frames of the real interface, filled with fictional tasks.
//!
//! `tt --screenshots [dir]` draws those frames and writes PNG files. Nothing
//! here talks to Todoist.

use std::io::{self, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use chrono::{NaiveDate, NaiveDateTime};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};
use serde_json::{Value, json};
use unicode_width::UnicodeWidthStr;

use crate::app::{AddForm, App, Focus, Overlay};
use crate::model::Editor;
use crate::todoist::{Due, Project, Task, User};
use crate::ui;

const COLS: u16 = 132;
const ROWS: u16 = 40;
const BG: [u8; 3] = [22, 22, 24];
const TEXT: [u8; 3] = [232, 230, 226];

const PAINTER: &str = r#"
import json, math, sys
from PIL import Image, ImageDraw, ImageFont

FONT = "/System/Library/Fonts/Menlo.ttc"
SIZE = 18

def main():
    data = json.load(sys.stdin)
    dest = sys.argv[1]
    regular = ImageFont.truetype(FONT, SIZE, index=0)
    bold = ImageFont.truetype(FONT, SIZE, index=1)
    advance = regular.getlength("M")
    ascent, descent = regular.getmetrics()
    cell_h = ascent + descent
    cols = data["cols"]
    rows = data["rows"]
    width = max(1, math.ceil(cols * advance))
    height = rows * cell_h
    image = Image.new("RGB", (width, height), (22, 22, 24))
    draw = ImageDraw.Draw(image)
    # Menlo's box-drawing glyphs stop short of the cell edge, so rules look dashed.
    # These arms meet the neighboring cell instead.
    arms = {
        "─": (1, 1, 0, 0),
        "│": (0, 0, 1, 1),
        "┌": (0, 1, 0, 1),
        "┐": (1, 0, 0, 1),
        "└": (0, 1, 1, 0),
        "┘": (1, 0, 1, 0),
        "├": (0, 1, 1, 1),
        "┤": (1, 0, 1, 1),
        "┬": (1, 1, 0, 1),
        "┴": (1, 1, 1, 0),
        "┼": (1, 1, 1, 1),
    }

    def stroke_box(symbol, x0, x1, y0, y1, color):
        spec = arms.get(symbol)
        if spec is None:
            return False
        left, right, up, down = spec
        left_x = math.floor(x0)
        right_x = math.ceil(x1)
        mid_x = (left_x + right_x) // 2
        mid_y = (y0 + y1) // 2
        if left or right:
            draw.line(
                [
                    (left_x if left else mid_x, mid_y),
                    ((right_x - 1) if right else mid_x, mid_y),
                ],
                fill=color,
            )
        if up or down:
            draw.line(
                [
                    (mid_x, y0 if up else mid_y),
                    (mid_x, (y1 - 1) if down else mid_y),
                ],
                fill=color,
            )
        return True

    cells = data["cells"]
    skip = 0
    for index, cell in enumerate(cells):
        if skip:
            skip -= 1
            continue
        span = max(1, int(cell["w"]))
        x = index % cols
        y = index // cols
        x0 = x * advance
        x1 = (x + span) * advance
        y0 = y * cell_h
        y1 = y0 + cell_h
        draw.rectangle([math.floor(x0), y0, math.ceil(x1) - 1, y1 - 1], fill=tuple(cell["bg"]))
        symbol = cell["c"]
        fg = tuple(cell["fg"])
        if not stroke_box(symbol, x0, x1, y0, y1, fg) and symbol != " ":
            face = bold if cell["b"] else regular
            draw.text((x0, y0 + ascent), symbol, font=face, fill=fg, anchor="ls")
        skip = span - 1
    cursor = data.get("cursor")
    if cursor:
        cx, cy = cursor
        x0 = cx * advance
        y0 = cy * cell_h
        draw.rectangle([x0, y0 + 2, x0 + 1, y0 + cell_h - 3], fill=(232, 230, 226))
    image.save(dest)

main()
"#;

pub fn write_all(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let scenes = [
        ("today", demo(false)),
        ("detail", detail_scene()),
        ("add-task", add_scene()),
        ("help", help_scene()),
    ];
    for (name, app) in scenes {
        let path = dir.join(format!("{name}.png"));
        paint(&capture_json(&app), &path)?;
        println!("{}", path.display());
    }
    Ok(())
}

fn detail_scene() -> App {
    let mut app = demo(true);
    app.focus = Focus::Detail;
    app
}

fn add_scene() -> App {
    let mut app = demo(false);
    app.overlay = Some(Overlay::Add(AddForm {
        text: Editor::new("Water the fern tomorrow p2 #Garden @home"),
        hint: "Adds to Inbox unless the text names a #project".to_string(),
        error: None,
        busy: false,
        generation: 0,
    }));
    app
}

fn help_scene() -> App {
    let mut app = demo(false);
    app.overlay = Some(Overlay::Help);
    app
}

fn capture_json(app: &App) -> String {
    let (buffer, cursor) = capture(app);
    let cells: Vec<Value> = buffer
        .content
        .iter()
        .map(|cell| {
            json!({
                "c": cell.symbol(),
                "fg": rgb(cell.fg, TEXT),
                "bg": rgb(cell.bg, BG),
                "b": cell.modifier.contains(Modifier::BOLD),
                "w": UnicodeWidthStr::width(cell.symbol()).max(1),
            })
        })
        .collect();
    json!({
        "cols": buffer.area.width,
        "rows": buffer.area.height,
        "cursor": cursor.map(|(x, y)| json!([x, y])),
        "cells": cells,
    })
    .to_string()
}

fn capture(app: &App) -> (Buffer, Option<(u16, u16)>) {
    let mut terminal = Terminal::new(TestBackend::new(COLS, ROWS)).expect("test terminal");
    terminal
        .draw(|frame| {
            ui::draw(frame, app);
        })
        .expect("draw");
    let backend = terminal.backend();
    let cursor = backend.cursor_visible().then(|| {
        let pos = backend.cursor_position();
        (pos.x, pos.y)
    });
    (backend.buffer().clone(), cursor)
}

fn rgb(color: Color, fallback: [u8; 3]) -> [u8; 3] {
    match color {
        Color::Rgb(red, green, blue) => [red, green, blue],
        _ => fallback,
    }
}

fn paint(json: &str, dest: &Path) -> io::Result<()> {
    let mut child = Command::new("python3")
        .arg("-c")
        .arg(PAINTER)
        .arg(dest)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| io::Error::new(err.kind(), format!("python3: {err}")))?;
    let written = child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("python3 stdin"))
        .and_then(|mut stdin| stdin.write_all(json.as_bytes()));
    let output = child.wait_with_output()?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(io::Error::other(format!(
            "painting {}: {err}",
            dest.display()
        )));
    }
    written?;
    Ok(())
}

#[allow(clippy::field_reassign_with_default)]
fn demo(long_note: bool) -> App {
    let mut app = App::default();
    app.now_override = Some(clock());
    app.user = Some(User {
        full_name: "Ada Quinn".to_string(),
        email: "ada@example.com".to_string(),
        inbox_project_id: "in".to_string(),
    });
    app.projects = vec![
        project("in", "Inbox", "grey", 0, None, true),
        project("home", "Home", "green", 1, None, false),
        project("reading", "Reading", "grape", 2, None, false),
        project("garden", "Garden", "teal", 3, None, false),
        project("herbs", "Herbs", "lime_green", 0, Some("garden"), false),
        project("weekend", "Weekend", "orange", 4, None, false),
    ];
    app.tasks = vec![
        task(
            "postcard",
            "in",
            "Mail the postcard",
            "The one with the harbor stamp.",
            1,
            Some(due("2026-10-13", "Oct 13", false)),
            -1,
            &[],
        ),
        task(
            "books",
            "reading",
            "Return the library books",
            &books_note(long_note),
            4,
            Some(due("2026-10-14", "Oct 14", false)),
            -1,
            &["errand"],
        ),
        task(
            "fern",
            "garden",
            "Repot the fern",
            "The cracked pot by the window can wait in the shed.",
            3,
            Some(due("2026-10-15", "yesterday", false)),
            -1,
            &[],
        ),
        task(
            "milk",
            "home",
            "Pick up oat milk",
            "The oat kind, from the shop on the corner.",
            2,
            Some(due("2026-10-16T17:00:00", "Today 5 PM", false)),
            0,
            &[],
        ),
        task(
            "herbs",
            "herbs",
            "Water the herbs",
            "",
            3,
            Some(due("2026-10-16", "every day", true)),
            1,
            &["water"],
        ),
        task(
            "notes",
            "weekend",
            "Sketch the Friday notes",
            "",
            1,
            Some(due("2026-10-16", "today", false)),
            2,
            &["writing"],
        ),
        task(
            "bookshop",
            "reading",
            "Call the bookshop",
            "Ask whether the field guide is back.",
            2,
            Some(due("2026-10-16T16:00:00", "Today 4 PM", false)),
            3,
            &[],
        ),
        task(
            "cake",
            "home",
            "Bake the apple cake",
            "",
            1,
            Some(due("2026-10-17", "tomorrow", false)),
            -1,
            &[],
        ),
        task(
            "market",
            "weekend",
            "Stop by the farmers market",
            "",
            1,
            Some(due("2026-10-18", "Saturday", false)),
            -1,
            &[],
        ),
        task(
            "chapter",
            "reading",
            "Finish the third chapter",
            "Stop after the scene on the train.",
            3,
            Some(due("2026-10-20", "Oct 20", false)),
            -1,
            &[],
        ),
        task(
            "idea",
            "in",
            "Idea for a kitchen window box",
            "",
            1,
            None,
            -1,
            &[],
        ),
    ];
    app.selected_id = Some("books".to_string());
    app.clamp_selection();
    app
}

fn books_note(long: bool) -> String {
    let mut lines = vec![
        "The left stack is the travel essays, and the blue one is still on the kitchen table.",
        "",
        "Drop them in the returns slot before the branch closes at six.",
        "",
        "CURRENT:",
        "two essay collections",
        "the field guide with the green cover",
        "the thin poetry book",
    ];
    if long {
        lines.extend([
            "",
            "The receipt is in the canvas bag by the door.",
            "Ask for a new card if the old one has expired.",
            "The branch is the one on Harbor Street.",
            "They close early on Friday.",
            "Leave the bag on the counter if the slot is full.",
        ]);
    }
    lines.join("\n")
}

fn clock() -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 10, 16)
        .unwrap()
        .and_hms_opt(9, 15, 0)
        .unwrap()
}

fn due(date: &str, text: &str, recurring: bool) -> Due {
    Due {
        date: date.to_string(),
        string: text.to_string(),
        is_recurring: recurring,
    }
}

fn project(
    id: &str,
    name: &str,
    color: &str,
    order: i64,
    parent: Option<&str>,
    inbox: bool,
) -> Project {
    Project {
        id: id.to_string(),
        name: name.to_string(),
        parent_id: parent.map(str::to_string),
        child_order: order,
        color: color.to_string(),
        is_inbox_project: inbox,
        is_archived: false,
        is_deleted: false,
    }
}

#[allow(clippy::too_many_arguments)]
fn task(
    id: &str,
    project_id: &str,
    content: &str,
    description: &str,
    priority: i32,
    due: Option<Due>,
    day_order: i64,
    labels: &[&str],
) -> Task {
    Task {
        id: id.to_string(),
        content: content.to_string(),
        description: description.to_string(),
        project_id: project_id.to_string(),
        section_id: None,
        parent_id: None,
        priority,
        labels: labels.iter().map(|label| label.to_string()).collect(),
        due,
        child_order: 0,
        day_order,
        note_count: 0,
        checked: false,
        is_deleted: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame_text(app: &App) -> String {
        let (buffer, _) = capture(app);
        let width = buffer.area.width as usize;
        let mut rows = Vec::new();
        for row in buffer.content.chunks(width) {
            let mut line = String::new();
            for cell in row {
                let symbol = cell.symbol();
                if UnicodeWidthStr::width(symbol) == 0 {
                    continue;
                }
                line.push_str(symbol);
            }
            rows.push(line);
        }
        rows.join("\n")
    }

    #[test]
    fn today_frame_uses_only_the_fictional_tasks() {
        let text = frame_text(&demo(false));
        assert!(text.contains("Ada Quinn"));
        assert!(text.contains("Return the library books"));
        assert!(text.contains("Oct 16 · Today · Friday"));
        assert!(text.contains("CURRENT:"));
        assert!(text.contains("two essay collections"));
        assert!(text.contains("Drop them in the returns slot"));
        assert!(!text.contains("table.Drop"));
        assert!(text.contains("My Projects"));
        assert!(text.contains("Herbs"));
        assert!(text.contains("every day"));
        assert!(!text.contains("Harbor Street"));
        assert!(!text.contains("─↓"));
        assert!(!text.contains("Sean"));
        assert!(!text.contains("TODOIST"));
        assert!(!text.contains("Coverage"));
    }

    #[test]
    fn detail_frame_scrolls_a_long_note() {
        let text = frame_text(&detail_scene());
        assert!(text.contains("─↓"));
        assert!(text.contains("↑↓ scroll"));
        assert!(text.contains("The receipt is in the canvas bag"));
    }

    #[test]
    fn add_and_help_frames_open_over_the_same_list() {
        let add = frame_text(&add_scene());
        assert!(add.contains("Add task"));
        assert!(add.contains("Water the fern tomorrow p2 #Garden @home"));
        assert!(add.contains("Return the library books"));

        let help = frame_text(&help_scene());
        assert!(help.contains("Keys"));
        assert!(help.contains("enter  >"));
        assert!(help.contains("Ada Quinn"));
    }
}
