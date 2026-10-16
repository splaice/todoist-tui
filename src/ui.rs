use std::collections::HashSet;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear};
use unicode_width::UnicodeWidthStr;

use crate::app::{AddForm, App, DeleteForm, DueForm, Focus, Overlay, ToastKind};
use crate::model::{
    HeaderKind, LineKind, NavItem, View, due_text, empty_message, entry_label, header_text,
    priority_name,
};
use crate::screen::{Hits, Screen, TaskHit};
use crate::text::{display_width, fit, wrap_parts};
use crate::theme::{self, Theme};
use crate::todoist::Task;

const HEADER_H: u16 = 4;

pub fn draw(frame: &mut Frame, app: &App) -> Screen {
    let theme = Theme::grok_night();
    let Some((mut screen, filter_area)) = paint_main(frame, app, theme) else {
        return Screen::default();
    };
    if let Some(overlay) = &app.overlay {
        match overlay {
            Overlay::Help => draw_help(frame, theme, &mut screen.hits),
            Overlay::Add(form) => draw_add(frame, form, theme, &mut screen.hits),
            Overlay::Due(form) => draw_due(frame, form, theme, &mut screen.hits),
            Overlay::Delete(form) => draw_delete(frame, form, theme, &mut screen.hits),
        }
    }
    place_cursor(frame, app, filter_area);
    screen
}

fn paint_main(frame: &mut Frame, app: &App, theme: Theme) -> Option<(Screen, Rect)> {
    let area = frame.area();
    let buf = frame.buffer_mut();
    fill(buf, area, theme.bg, theme.text);
    if let Some(message) = &app.fatal {
        draw_fatal(buf, area, theme, message);
        return None;
    }
    if area.width < 64 || area.height < 16 {
        put(
            buf,
            area.x + 1,
            area.y + 1,
            area.right(),
            "Terminal too small",
            theme.text(),
        );
        put(
            buf,
            area.x + 1,
            area.y + 3,
            area.right(),
            "q quit",
            theme.dim(),
        );
        return None;
    }

    let [main, status] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
    let left_w = ((main.width as u32 * 28) / 100).clamp(24, 36) as u16;
    let left_w = left_w.min(main.width.saturating_sub(40));
    let [left, divider, right] = Layout::horizontal([
        Constraint::Length(left_w),
        Constraint::Length(1),
        Constraint::Min(20),
    ])
    .areas(main);
    for y in divider.y..divider.bottom() {
        buf[(divider.x, y)]
            .set_symbol("│")
            .set_style(theme.fg(theme.border));
    }
    let [left_head, left_body] =
        Layout::vertical([Constraint::Length(HEADER_H), Constraint::Min(1)]).areas(left);
    let [right_head, right_body] =
        Layout::vertical([Constraint::Length(HEADER_H), Constraint::Min(1)]).areas(right);
    let rule_y = left_head.bottom().saturating_sub(1);
    hline(buf, left_head.x, rule_y, left_head.width, theme);
    hline(buf, right_head.x, rule_y, right_head.width, theme);
    buf[(divider.x, rule_y)]
        .set_symbol("┼")
        .set_style(theme.fg(theme.border));

    let mut hits = Hits::default();
    draw_account(buf, left_head, app, theme, &mut hits);
    draw_view_header(buf, right_head, app, theme, &mut hits);

    hits.nav_area = Some(left_body);
    let (nav_offset, nav_len) = draw_nav(buf, left_body, app, theme, &mut hits);

    let filter_area = Rect {
        x: right_body.x,
        y: right_body.y,
        width: right_body.width,
        height: 1,
    };
    let rest = Rect {
        x: right_body.x,
        y: right_body.y.saturating_add(1),
        width: right_body.width,
        height: right_body.height.saturating_sub(1),
    };
    let content_lines = {
        let lines = app.line_layout();
        if lines
            .iter()
            .any(|line| matches!(line, LineKind::TaskTop(_)))
        {
            lines.len()
        } else {
            0
        }
    };
    let (list_area, rule, detail_area) = split_list_detail(rest, content_lines);
    hits.filter = Some(filter_area);
    hits.task_area = Some(list_area);
    hits.detail_area = (detail_area.height > 0).then_some(detail_area);
    draw_filter(buf, filter_area, app, theme);
    let (task_scroll, task_len) = draw_tasks(buf, list_area, app, theme, &mut hits);
    let (detail_rows, detail_height) = if let Some(rule) = rule {
        let rule_style = if app.focus == Focus::Detail {
            theme.fg(theme.accent)
        } else {
            theme.fg(theme.border)
        };
        hline(buf, rule.x, rule.y, rule.width, theme);
        for col in rule.x..rule.x.saturating_add(rule.width) {
            buf[(col, rule.y)].set_style(rule_style);
        }
        buf[(divider.x, rule.y)]
            .set_symbol("├")
            .set_style(rule_style);
        let (rows, height) = draw_detail_pane(buf, detail_area, app, theme);
        let offset = app.detail_scroll.min(rows.saturating_sub(height));
        paint_scroll_marks(buf, rule, offset, rows, height, theme);
        (rows, height)
    } else {
        (0, 0)
    };

    draw_status(buf, status, app, theme);

    Some((
        Screen {
            hits,
            nav_height: left_body.height as usize,
            nav_offset,
            nav_len,
            task_height: list_area.height as usize,
            task_scroll,
            task_len,
            detail_rows,
            detail_height,
        },
        filter_area,
    ))
}

/// The list keeps only the rows it needs, and never more than 70% of `rest`.
/// The detail pane fills every remaining row and stays at least 30% tall.
/// The rule between them is taken from the list's share.
fn split_list_detail(rest: Rect, content_lines: usize) -> (Rect, Option<Rect>, Rect) {
    let space = rest.height;
    if space == 0 {
        let empty = Rect {
            x: rest.x,
            y: rest.y,
            width: rest.width,
            height: 0,
        };
        return (empty, None, empty);
    }
    let detail_min = at_least_percent(space, 30).min(space);
    let rule_h = u16::from(space > detail_min);
    let list_cap = space.saturating_sub(detail_min + rule_h);
    let wanted = if content_lines == 0 {
        list_cap.min(2)
    } else {
        u16::try_from(content_lines).unwrap_or(u16::MAX)
    };
    let list_h = wanted.min(list_cap);
    if list_h == 0 {
        let list = Rect {
            x: rest.x,
            y: rest.y,
            width: rest.width,
            height: 0,
        };
        return (list, None, rest);
    }
    let rule = if rule_h == 1 {
        Some(Rect {
            x: rest.x,
            y: rest.y + list_h,
            width: rest.width,
            height: 1,
        })
    } else {
        None
    };
    let detail_h = space - list_h - rule_h;
    let detail = Rect {
        x: rest.x,
        y: rest.y + list_h + rule_h,
        width: rest.width,
        height: detail_h,
    };
    let list = Rect {
        x: rest.x,
        y: rest.y,
        width: rest.width,
        height: list_h,
    };
    (list, rule, detail)
}

fn at_least_percent(space: u16, percent: u16) -> u16 {
    u32::from(space)
        .saturating_mul(u32::from(percent))
        .div_ceil(100) as u16
}

fn draw_fatal(buf: &mut ratatui::buffer::Buffer, area: Rect, theme: Theme, message: &str) {
    put(
        buf,
        area.x + 2,
        area.y + 2,
        area.right(),
        "tt",
        theme.bold_accent(),
    );
    let mut y = area.y + 4;
    for part in wrap_parts(message, area.width.saturating_sub(4) as usize) {
        if y >= area.bottom().saturating_sub(2) {
            break;
        }
        put(
            buf,
            area.x + 2,
            y,
            area.right().saturating_sub(1),
            &part,
            theme.text(),
        );
        y += 1;
    }
    put(
        buf,
        area.x + 2,
        y.saturating_add(1),
        area.right(),
        "q  quit",
        theme.dim(),
    );
}

fn draw_account(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    app: &App,
    theme: Theme,
    hits: &mut Hits,
) {
    let max_x = area.right().saturating_sub(1);
    let label = if app.focus == Focus::Nav {
        theme.accent()
    } else {
        theme.faint()
    };
    put(buf, area.x + 1, area.y, max_x, "views", label);
    let title = if app.user.is_none() && app.loading {
        "Loading…".to_string()
    } else {
        app.account_label()
    };
    put(buf, area.x + 1, area.y + 1, max_x, &title, theme.bold());

    let y = area.y + 2;
    let (add, next) = action(buf, area.x + 1, y, max_x, "n", "add", true, false, theme);
    hits.add = Some(add);
    let refresh_x = next.saturating_add(3);
    let (refresh, _) = action(buf, refresh_x, y, max_x, "r", "refresh", true, false, theme);
    hits.refresh = Some(refresh);
}

fn draw_view_header(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    app: &App,
    theme: Theme,
    hits: &mut Hits,
) {
    let max_x = area.right().saturating_sub(1);
    let label = if matches!(app.focus, Focus::Tasks | Focus::Filter) {
        theme.accent()
    } else {
        theme.faint()
    };
    put(buf, area.x + 1, area.y, max_x, app.view.eyebrow(), label);
    let count = header_count(app);
    let count_w = UnicodeWidthStr::width(count.as_str()) as u16;
    let count_x = area.right().saturating_sub(1 + count_w);
    if count_x > area.x + 8 {
        put(buf, count_x, area.y, area.right(), &count, theme.faint());
    }
    put(
        buf,
        area.x + 1,
        area.y + 1,
        max_x,
        &app.view_title(),
        theme.bold(),
    );

    let y = area.y + 2;
    let ready = app.selected_task().is_some();
    let (done, next) = action(buf, area.x + 1, y, max_x, "x", "done", ready, false, theme);
    hits.done = ready.then_some(done);
    let (due, next) = action(
        buf,
        next.saturating_add(3),
        y,
        max_x,
        "e",
        "due",
        ready,
        false,
        theme,
    );
    hits.due = ready.then_some(due);
    let (delete, next) = action(
        buf,
        next.saturating_add(3),
        y,
        max_x,
        "d",
        "delete",
        ready,
        true,
        theme,
    );
    hits.delete = ready.then_some(delete);
    if app.saving() {
        let note = "saving";
        let note_x = area
            .right()
            .saturating_sub(1 + UnicodeWidthStr::width(note) as u16);
        if note_x > next + 1 {
            put(buf, note_x, y, max_x, note, theme.faint());
        }
    } else if app.loading && !app.tasks.is_empty() {
        let note = "loading";
        let note_x = area
            .right()
            .saturating_sub(1 + UnicodeWidthStr::width(note) as u16);
        if note_x > next + 1 {
            put(buf, note_x, y, max_x, note, theme.faint());
        }
    }
}

fn header_count(app: &App) -> String {
    if app.loading && app.tasks.is_empty() {
        return "…".to_string();
    }
    let total = app.view_count();
    if app.filter.value.trim().is_empty() {
        total.to_string()
    } else {
        format!("{}/{}", app.shown_count(), total)
    }
}

fn draw_nav(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    app: &App,
    theme: Theme,
    hits: &mut Hits,
) -> (usize, usize) {
    let rows = app.navigation();
    let height = area.height as usize;
    let offset = app.nav_offset.min(rows.len().saturating_sub(height));
    let max_x = area.right().saturating_sub(1);
    for (index, row) in rows.iter().enumerate().skip(offset).take(height) {
        let y = area.y + (index - offset) as u16;
        let rect = Rect {
            x: area.x,
            y,
            width: area.width,
            height: 1,
        };
        match &row.item {
            NavItem::Blank => {}
            NavItem::Header => {
                put(buf, area.x + 1, y, max_x, "My Projects", theme.faint());
            }
            NavItem::Entry(view) => {
                let selected = view == &app.view;
                let focused = selected && app.focus == Focus::Nav;
                if selected {
                    let bg = if focused {
                        theme.bg_selected
                    } else {
                        theme.bg_field
                    };
                    fill(buf, rect, bg, theme.text);
                }
                let count = row.count.map(|count| count.to_string()).unwrap_or_default();
                let count_w = UnicodeWidthStr::width(count.as_str()) as u16;
                let count_x = max_x.saturating_sub(count_w);
                let base = area.x + 1 + row.depth as u16 * 2;
                let name_x = if matches!(view, View::Project(_)) {
                    let project = match view {
                        View::Project(id) => app.project_named(id),
                        _ => None,
                    };
                    let color = project
                        .map(|project| theme::project_color(&project.color))
                        .unwrap_or(theme.dim);
                    put(buf, base, y, max_x, "#", theme.fg(color));
                    base.saturating_add(2)
                } else {
                    base.saturating_add(2)
                };
                let name_end = count_x.saturating_sub(1);
                let name_style = if focused {
                    theme.bold_accent()
                } else if selected {
                    theme.bold()
                } else {
                    theme.text()
                };
                let bg = if focused {
                    Some(theme.bg_selected)
                } else if selected {
                    Some(theme.bg_field)
                } else {
                    None
                };
                put(
                    buf,
                    name_x,
                    y,
                    name_end,
                    &entry_label(view, &app.projects),
                    with_bg(name_style, bg),
                );
                if count_w > 0 && count_x > name_x {
                    put(
                        buf,
                        count_x,
                        y,
                        area.right(),
                        &count,
                        with_bg(theme.faint(), bg),
                    );
                }
                hits.nav_rows.push((rect, index));
            }
        }
    }
    (offset, rows.len())
}

fn draw_filter(buf: &mut ratatui::buffer::Buffer, area: Rect, app: &App, theme: Theme) {
    let max_x = area.right().saturating_sub(1);
    let focused = app.focus == Focus::Filter && app.overlay.is_none();
    let slash = if focused {
        theme.bold_accent()
    } else {
        theme.faint()
    };
    put(buf, area.x + 1, area.y, max_x, "/", slash);
    if app.filter.value.is_empty() && !focused {
        put(buf, area.x + 3, area.y, max_x, "filter", theme.faint());
    } else {
        let room = max_x.saturating_sub(area.x + 3) as usize;
        let (visible, _) = app.filter.window(room);
        put(buf, area.x + 3, area.y, max_x, &visible, theme.text());
    }
}

fn draw_tasks(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    app: &App,
    theme: Theme,
    hits: &mut Hits,
) -> (usize, usize) {
    let lines = app.line_layout();
    let has_tasks = lines
        .iter()
        .any(|line| matches!(line, LineKind::TaskTop(_)));
    let height = area.height as usize;
    if !has_tasks {
        draw_empty(buf, area, app, theme);
        return (0, 0);
    }

    let parents = visible_parents(app);
    let offset = app.task_scroll.min(lines.len().saturating_sub(height));
    let max_x = area.right().saturating_sub(1);
    for (index, line) in lines.iter().enumerate().skip(offset).take(height) {
        let y = area.y + (index - offset) as u16;
        let rect = Rect {
            x: area.x,
            y,
            width: area.width,
            height: 1,
        };
        match line {
            LineKind::Blank => {}
            LineKind::Header(kind) => draw_header(buf, rect, *kind, app, theme),
            LineKind::TaskTop(task_index) => {
                draw_task_top(buf, rect, *task_index, app, &parents, theme, max_x);
                hits.task_rows.push((rect, TaskHit::Task(*task_index)));
            }
            LineKind::TaskBody(task_index) => {
                draw_task_body(buf, rect, *task_index, app, &parents, theme, max_x);
                hits.task_rows.push((rect, TaskHit::Task(*task_index)));
            }
        }
    }
    (offset, lines.len())
}

fn draw_empty(buf: &mut ratatui::buffer::Buffer, area: Rect, app: &App, theme: Theme) {
    let (message, style) = if let Some(err) = &app.load_error
        && app.tasks.is_empty()
    {
        (err.clone(), theme.fg(theme.danger))
    } else if app.loading && app.tasks.is_empty() {
        ("Loading tasks…".to_string(), theme.dim())
    } else {
        (
            empty_message(&app.view, !app.filter.value.trim().is_empty()).to_string(),
            theme.dim(),
        )
    };
    let width = area.width.saturating_sub(2) as usize;
    for (offset, part) in wrap_parts(&message, width).into_iter().take(2).enumerate() {
        let y = area.y + offset as u16;
        if y >= area.bottom() {
            break;
        }
        put(
            buf,
            area.x + 1,
            y,
            area.right().saturating_sub(1),
            &part,
            style,
        );
    }
}

fn draw_header(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    kind: HeaderKind,
    app: &App,
    theme: Theme,
) {
    let text = header_text(kind, app.today());
    let style = match kind {
        HeaderKind::Overdue => theme.fg(theme.danger).add_modifier(Modifier::BOLD),
        HeaderKind::NoDate => theme.faint(),
        HeaderKind::Date(_) => theme.bold(),
    };
    put(
        buf,
        area.x + 1,
        area.y,
        area.right().saturating_sub(1),
        &text,
        style,
    );
    if kind == HeaderKind::Overdue {
        let hint = "e due";
        let hint_x = area
            .right()
            .saturating_sub(1 + UnicodeWidthStr::width(hint) as u16);
        let title_w = UnicodeWidthStr::width(text.as_str()) as u16;
        if hint_x > area.x + 2 + title_w {
            put(buf, hint_x, area.y, area.right(), hint, theme.faint());
        }
    }
}

fn draw_task_top(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    index: usize,
    app: &App,
    parents: &HashSet<String>,
    theme: Theme,
    max_x: u16,
) {
    let Some(task) = app.tasks.get(index) else {
        return;
    };
    let (selected, bg) = selection(app, task);
    if let Some(bg) = bg {
        fill(buf, area, bg, theme.text);
    }
    let indent = indent_of(task, &app.view, parents);
    let mark_x = area.x + 1 + indent;
    let (glyph, color) = if app.closing.contains(&task.id) {
        ("✓", theme.ok)
    } else {
        priority_mark(theme, task.priority)
    };
    put(
        buf,
        mark_x,
        area.y,
        max_x,
        glyph,
        with_bg(theme.fg(color), bg),
    );
    let content_style = if app.closing.contains(&task.id) {
        theme.dim()
    } else if selected && app.focus == Focus::Tasks {
        theme.bold_accent()
    } else if selected {
        theme.bold()
    } else {
        theme.text()
    };
    put(
        buf,
        mark_x.saturating_add(2),
        area.y,
        max_x,
        &task.content,
        with_bg(content_style, bg),
    );
}

fn draw_task_body(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    index: usize,
    app: &App,
    parents: &HashSet<String>,
    theme: Theme,
    max_x: u16,
) {
    let Some(task) = app.tasks.get(index) else {
        return;
    };
    let (_, bg) = selection(app, task);
    if let Some(bg) = bg {
        fill(buf, area, bg, theme.text);
    }
    let indent = indent_of(task, &app.view, parents);
    let x = area.x + 1 + indent + 2;
    let (project, project_color) = right_project(app, task);
    let notes = (task.note_count > 0).then(|| task.note_count.to_string());
    let right_w = display_width(project.as_deref().unwrap_or(""))
        + notes
            .as_ref()
            .map(|notes| display_width(notes) + 2)
            .unwrap_or(0);
    let due = task
        .due
        .as_ref()
        .map(|due| due_text(due, app.now()))
        .filter(|due| !due.label.is_empty());
    let due_label = due.as_ref().map(|due| due.label.as_str()).unwrap_or("");
    let due_w = display_width(due_label);
    let gap = if due_w == 0 { 0 } else { 1 };
    let detail = task_detail(task);
    let detail_x = x.saturating_add((due_w + gap) as u16);
    let reserve = if right_w == 0 { 0 } else { right_w + 2 };
    let detail_end = max_x.saturating_sub(reserve as u16);
    if let Some(due) = &due {
        let on_today = task.due.as_ref().and_then(crate::model::calendar_day) == Some(app.today());
        let style = if due.overdue {
            theme.fg(theme.danger)
        } else if on_today {
            theme.fg(theme.warn)
        } else {
            theme.dim()
        };
        put(buf, x, area.y, max_x, &due.label, with_bg(style, bg));
    }
    if detail_x < detail_end && !detail.is_empty() {
        put(
            buf,
            detail_x,
            area.y,
            detail_end,
            &detail,
            with_bg(theme.faint(), bg),
        );
    }
    let mut cursor = max_x.saturating_sub(right_w as u16);
    if let Some(notes) = notes
        && cursor < max_x
    {
        let rect = put(
            buf,
            cursor,
            area.y,
            max_x,
            &notes,
            with_bg(theme.faint(), bg),
        );
        cursor = rect.right().saturating_add(2);
    }
    if let Some(project) = project
        && cursor < max_x
    {
        put(
            buf,
            cursor,
            area.y,
            area.right(),
            &project,
            with_bg(theme.fg(project_color), bg),
        );
    }
}

fn selection(app: &App, task: &Task) -> (bool, Option<ratatui::style::Color>) {
    let selected = app.selected_id.as_deref() == Some(task.id.as_str());
    if !selected {
        return (false, None);
    }
    let bg = if app.focus == Focus::Tasks {
        Theme::grok_night().bg_selected
    } else {
        Theme::grok_night().bg_field
    };
    (true, Some(bg))
}

fn visible_parents(app: &App) -> HashSet<String> {
    app.filtered_indices()
        .into_iter()
        .filter_map(|index| app.tasks.get(index).map(|task| task.id.clone()))
        .collect()
}

fn indent_of(task: &Task, view: &View, parents: &HashSet<String>) -> u16 {
    if matches!(view, View::Inbox | View::Project(_))
        && task.parent().is_some_and(|id| parents.contains(id))
    {
        2
    } else {
        0
    }
}

fn priority_mark(theme: Theme, priority: i32) -> (&'static str, ratatui::style::Color) {
    match priority {
        4 => ("●", theme.p1),
        3 => ("●", theme.p2),
        2 => ("●", theme.p3),
        _ => ("○", theme.faint),
    }
}

fn task_detail(task: &Task) -> String {
    let description = task.description.trim();
    if !description.is_empty() {
        return description.to_string();
    }
    if task.labels.is_empty() {
        return String::new();
    }
    task.labels
        .iter()
        .map(|label| format!("@{label}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn right_project(app: &App, task: &Task) -> (Option<String>, ratatui::style::Color) {
    if !matches!(app.view, View::Today | View::Upcoming) {
        return (None, Theme::grok_night().dim);
    }
    let Some(project) = app.project_named(&task.project_id) else {
        return (None, Theme::grok_night().dim);
    };
    if project.name.is_empty() {
        return (None, Theme::grok_night().dim);
    }
    (
        Some(project.name.clone()),
        theme::project_color(&project.color),
    )
}

fn draw_status(buf: &mut ratatui::buffer::Buffer, area: Rect, app: &App, theme: Theme) {
    fill(buf, area, theme.bg_raised, theme.dim);
    let toast = app.toast.as_ref().map(|toast| {
        let style = match toast.kind {
            ToastKind::Info => theme.fg(theme.accent),
            ToastKind::Err => theme.fg(theme.danger),
        };
        (toast.text.clone(), style)
    });
    let right = toast.unwrap_or_else(|| (status_count(app), theme.faint()));
    let right_w = UnicodeWidthStr::width(right.0.as_str()) as u16;
    let right_x = area.right().saturating_sub(1 + right_w);
    draw_hints(
        buf,
        area.x + 1,
        area.y,
        right_x.saturating_sub(2),
        app,
        theme,
    );
    if right_x > area.x {
        put(buf, right_x, area.y, area.right(), &right.0, right.1);
    }
}

fn status_count(app: &App) -> String {
    if app.loading && app.tasks.is_empty() {
        return "loading".to_string();
    }
    if app.saving() {
        return "saving".to_string();
    }
    if app.loading {
        return "refreshing".to_string();
    }
    let total = app.view_count();
    let noun = if total == 1 { "task" } else { "tasks" };
    if app.filter.value.trim().is_empty() {
        format!("{total} {noun}")
    } else {
        format!("{} of {total}", app.shown_count())
    }
}

fn draw_hints(
    buf: &mut ratatui::buffer::Buffer,
    mut x: u16,
    y: u16,
    max_x: u16,
    app: &App,
    theme: Theme,
) {
    let hints: &[(&str, bool)] = match (&app.overlay, app.focus) {
        (Some(Overlay::Help), _) => &[
            ("esc", true),
            (" close", false),
            ("   ", false),
            ("q", true),
            (" quit", false),
        ],
        (Some(Overlay::Add(_)), _) => &[
            ("enter", true),
            (" add", false),
            ("   ", false),
            ("esc", true),
            (" cancel", false),
        ],
        (Some(Overlay::Due(_)), _) => &[
            ("enter", true),
            (" save", false),
            ("   ", false),
            ("esc", true),
            (" cancel", false),
        ],
        (Some(Overlay::Delete(_)), _) => &[
            ("enter", true),
            (" delete", false),
            ("   ", false),
            ("esc", true),
            (" cancel", false),
        ],
        (_, Focus::Filter) => &[
            ("type to filter", false),
            ("   ", false),
            ("↑↓", true),
            (" tasks", false),
            ("   ", false),
            ("enter", true),
            (" done", false),
            ("   ", false),
            ("esc", true),
            (" clear", false),
        ],
        (_, Focus::Tasks) => &[
            ("↑↓", true),
            (" move", false),
            ("   ", false),
            ("x", true),
            (" done", false),
            ("   ", false),
            ("e", true),
            (" due", false),
            ("   ", false),
            ("enter", true),
            (" detail", false),
            ("   ", false),
            ("/", true),
            (" filter", false),
            ("   ", false),
            ("tab", true),
            (" nav", false),
            ("   ", false),
            ("?", true),
            (" help", false),
        ],
        (_, Focus::Detail) => &[
            ("↑↓", true),
            (" scroll", false),
            ("   ", false),
            ("esc", true),
            (" back", false),
            ("   ", false),
            ("g G", true),
            (" ends", false),
        ],
        (_, Focus::Nav) => &[
            ("↑↓", true),
            (" move", false),
            ("   ", false),
            ("/", true),
            (" filter", false),
            ("   ", false),
            ("tab", true),
            (" tasks", false),
            ("   ", false),
            ("n", true),
            (" add", false),
            ("   ", false),
            ("r", true),
            (" refresh", false),
            ("   ", false),
            ("?", true),
            (" help", false),
        ],
    };
    for (text, key) in hints {
        if x >= max_x {
            break;
        }
        let style = if *key { theme.accent() } else { theme.dim() };
        let rect = put(buf, x, y, max_x, text, style);
        if rect.width == 0 {
            break;
        }
        x = rect.right();
    }
}

fn draw_help(frame: &mut Frame, theme: Theme, hits: &mut Hits) {
    let area = centered(frame.area(), 62, 20);
    hits.modal = Some(area);
    let block = Block::default()
        .title(Line::from(Span::styled(" Keys ", theme.bold_accent())))
        .borders(Borders::ALL)
        .border_style(theme.fg(theme.accent))
        .style(Style::new().bg(theme.bg_raised).fg(theme.text));
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    let buf = frame.buffer_mut();
    fill(buf, inner, theme.bg_raised, theme.text);
    let rows = [
        ("Move", "↑ ↓    j k"),
        ("Page", "pgup pgdn    ctrl-u d"),
        ("Jump", "g G    home end"),
        ("Panes", "tab    h l    ← →"),
        ("Filter", "/"),
        ("Add task", "n"),
        ("Complete", "x    space"),
        ("Reschedule", "e"),
        ("Delete", "d"),
        ("Detail", "enter  >    ↑↓ when open"),
        ("Refresh", "r"),
        ("Quick add", "tomorrow  #Project  p1  @label"),
        ("Close", "esc"),
        ("Quit", "q    ctrl-c"),
    ];
    for (index, (label, keys)) in rows.iter().enumerate() {
        let y = inner.y + index as u16;
        if y >= inner.bottom() {
            break;
        }
        put(buf, inner.x + 1, y, inner.x + 16, label, theme.dim());
        put(
            buf,
            inner.x + 16,
            y,
            inner.right().saturating_sub(1),
            keys,
            theme.text(),
        );
    }
}

const ADD_SYNTAX: &str = "tomorrow, #Project, p1, @label, // note";

struct AddModal {
    width: u16,
    height: u16,
    hint: Vec<String>,
    syntax: Vec<String>,
    status: Vec<String>,
    status_error: bool,
}

/// Tall enough for the hint, syntax, field, and status without scrolling,
/// and never taller than 80% of the screen.
fn add_modal(area: Rect, hint: &str, error: Option<&str>, busy: bool) -> AddModal {
    let width = 68.min(area.width).max(1);
    let text_w = width.saturating_sub(4).max(1) as usize;
    let hint_lines = wrap_or_one(hint, text_w);
    let syntax_lines = wrap_or_one(ADD_SYNTAX, text_w);
    let (status, status_error) = if let Some(error) = error {
        (wrap_or_one(error, text_w), true)
    } else if busy {
        (vec!["Saving…".to_string()], false)
    } else {
        (Vec::new(), false)
    };
    let mut inner = hint_lines.len() + 1 + syntax_lines.len() + 1 + 1;
    if !status.is_empty() {
        inner += 1 + status.len();
    }
    let needed = (inner as u16).saturating_add(2);
    let max_h = (u32::from(area.height) * 80 / 100).max(1) as u16;
    let height = needed.min(max_h).min(area.height.max(1));
    AddModal {
        width,
        height,
        hint: hint_lines,
        syntax: syntax_lines,
        status,
        status_error,
    }
}

fn wrap_or_one(text: &str, width: usize) -> Vec<String> {
    let lines = wrap_parts(text, width);
    if lines.is_empty() {
        vec![String::new()]
    } else {
        lines
    }
}

fn draw_add(frame: &mut Frame, form: &AddForm, theme: Theme, hits: &mut Hits) {
    let modal = add_modal(frame.area(), &form.hint, form.error.as_deref(), form.busy);
    let area = centered(frame.area(), modal.width, modal.height);
    hits.modal = Some(area);
    let block = Block::default()
        .title(Line::from(Span::styled(" Add task ", theme.bold_accent())))
        .borders(Borders::ALL)
        .border_style(theme.fg(theme.accent))
        .style(Style::new().bg(theme.bg_raised).fg(theme.text));
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    let cursor = {
        let buf = frame.buffer_mut();
        fill(buf, inner, theme.bg_raised, theme.text);
        let max_x = inner.right().saturating_sub(1);
        let mut y = inner.y;
        for line in &modal.hint {
            if y >= inner.bottom() {
                break;
            }
            put(buf, inner.x + 1, y, max_x, line, theme.dim());
            y = y.saturating_add(1);
        }
        y = y.saturating_add(1);
        for line in &modal.syntax {
            if y >= inner.bottom() {
                break;
            }
            put(buf, inner.x + 1, y, max_x, line, theme.faint());
            y = y.saturating_add(1);
        }
        y = y.saturating_add(1);
        let field_y = y;
        let field = Rect {
            x: inner.x + 1,
            y: field_y,
            width: inner.width.saturating_sub(2),
            height: 1,
        };
        let col = if field_y < inner.bottom() && field.width > 0 {
            fill(buf, field, theme.bg_field, theme.text);
            let room = field.width.saturating_sub(1) as usize;
            let (visible, col) = form.text.window(room);
            put(buf, field.x, field.y, field.right(), &visible, theme.text());
            col
        } else {
            0
        };
        y = field_y.saturating_add(2);
        let status_style = if modal.status_error {
            theme.fg(theme.danger)
        } else {
            theme.faint()
        };
        for line in &modal.status {
            if y >= inner.bottom() {
                break;
            }
            put(buf, inner.x + 1, y, max_x, line, status_style);
            y = y.saturating_add(1);
        }
        (field_y < inner.bottom() && !form.busy).then_some((field.x + col as u16, field_y))
    };
    if let Some(position) = cursor {
        frame.set_cursor_position(position);
    }
}

fn draw_due(frame: &mut Frame, form: &DueForm, theme: Theme, hits: &mut Hits) {
    let area = centered(frame.area(), 68, 11);
    hits.modal = Some(area);
    let block = Block::default()
        .title(Line::from(Span::styled(
            " Reschedule ",
            theme.bold_accent(),
        )))
        .borders(Borders::ALL)
        .border_style(theme.fg(theme.accent))
        .style(Style::new().bg(theme.bg_raised).fg(theme.text));
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    let cursor = {
        let buf = frame.buffer_mut();
        fill(buf, inner, theme.bg_raised, theme.text);
        let max_x = inner.right().saturating_sub(1);
        put(
            buf,
            inner.x + 1,
            inner.y,
            max_x,
            &form.content,
            theme.bold(),
        );
        put(buf, inner.x + 1, inner.y + 2, max_x, "Due", theme.dim());
        let field = Rect {
            x: inner.x + 1,
            y: inner.y + 3,
            width: inner.width.saturating_sub(2),
            height: 1,
        };
        fill(buf, field, theme.bg_field, theme.text);
        let room = field.width.saturating_sub(1) as usize;
        let (visible, col) = form.due.window(room);
        let shown = if visible.is_empty() && !form.busy {
            "tomorrow, friday, oct 21".to_string()
        } else {
            visible
        };
        let style = if form.due.value.is_empty() {
            theme.faint()
        } else {
            theme.text()
        };
        put(buf, field.x, field.y, field.right(), &shown, style);
        if let Some(err) = &form.error {
            put(
                buf,
                inner.x + 1,
                inner.y + 5,
                max_x,
                err,
                theme.fg(theme.danger),
            );
        } else if form.busy {
            put(
                buf,
                inner.x + 1,
                inner.y + 5,
                max_x,
                "Saving…",
                theme.faint(),
            );
        }
        (!form.busy).then_some((field.x + col as u16, field.y))
    };
    if let Some(position) = cursor {
        frame.set_cursor_position(position);
    }
}

fn draw_delete(frame: &mut Frame, form: &DeleteForm, theme: Theme, hits: &mut Hits) {
    let area = centered(frame.area(), 68, 9);
    hits.modal = Some(area);
    let block = Block::default()
        .title(Line::from(Span::styled(
            " Delete task ",
            theme.fg(theme.danger).add_modifier(Modifier::BOLD),
        )))
        .borders(Borders::ALL)
        .border_style(theme.fg(theme.danger))
        .style(Style::new().bg(theme.bg_raised).fg(theme.text));
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    let buf = frame.buffer_mut();
    fill(buf, inner, theme.bg_raised, theme.text);
    let max_x = inner.right().saturating_sub(1);
    put(
        buf,
        inner.x + 1,
        inner.y,
        max_x,
        &form.content,
        theme.bold(),
    );
    put(
        buf,
        inner.x + 1,
        inner.y + 2,
        max_x,
        "This cannot be undone.",
        theme.dim(),
    );
    if let Some(err) = &form.error {
        put(
            buf,
            inner.x + 1,
            inner.y + 4,
            max_x,
            err,
            theme.fg(theme.danger),
        );
    } else if form.busy {
        put(
            buf,
            inner.x + 1,
            inner.y + 4,
            max_x,
            "Deleting…",
            theme.faint(),
        );
    }
}

fn draw_detail_pane(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    app: &App,
    theme: Theme,
) -> (usize, usize) {
    if area.height == 0 {
        return (0, 0);
    }
    fill(buf, area, theme.bg_raised, theme.text);
    let width = area.width.saturating_sub(2) as usize;
    let lines = if app.selected_task().is_none() {
        vec![("Select a task".to_string(), theme.dim())]
    } else {
        let id = app.selected_id.as_deref().unwrap_or("");
        detail_lines(app, id, width.max(1), theme)
    };
    let height = area.height as usize;
    let offset = app.detail_scroll.min(lines.len().saturating_sub(height));
    for (index, (text, style)) in lines.iter().enumerate().skip(offset).take(height) {
        let y = area.y + (index - offset) as u16;
        put(
            buf,
            area.x + 1,
            y,
            area.right().saturating_sub(1),
            text,
            *style,
        );
    }
    (lines.len(), height)
}

fn paint_scroll_marks(
    buf: &mut ratatui::buffer::Buffer,
    rule: Rect,
    offset: usize,
    rows: usize,
    height: usize,
    theme: Theme,
) {
    let mut mark = String::new();
    if offset > 0 {
        mark.push('↑');
    }
    if offset + height < rows {
        if !mark.is_empty() {
            mark.push(' ');
        }
        mark.push('↓');
    }
    if mark.is_empty() || rule.width < 4 {
        return;
    }
    let width = display_width(&mark) as u16;
    let x = rule.right().saturating_sub(1 + width);
    if x > rule.x {
        put(buf, x, rule.y, rule.right(), &mark, theme.faint());
    }
}

fn detail_lines(app: &App, id: &str, width: usize, theme: Theme) -> Vec<(String, Style)> {
    let Some(task) = app.tasks.iter().find(|task| task.id == id) else {
        return vec![("Task is gone".to_string(), theme.dim())];
    };
    let mut lines = Vec::new();
    let title = if app.focus == Focus::Detail {
        theme.bold_accent()
    } else {
        theme.bold()
    };
    for part in wrap_parts(&task.content, width) {
        lines.push((part, title));
    }
    lines.push((String::new(), theme.text()));
    let mut meta = Vec::new();
    if let Some(project) = app.project_named(&task.project_id) {
        meta.push(project.name.clone());
    }
    meta.push(priority_name(task.priority).to_string());
    if let Some(due) = &task.due {
        let text = due_text(due, app.now());
        if !text.label.is_empty() {
            meta.push(text.label);
        }
    }
    lines.push((meta.join("  ·  "), theme.dim()));
    if !task.labels.is_empty() {
        let labels = task
            .labels
            .iter()
            .map(|label| format!("@{label}"))
            .collect::<Vec<_>>()
            .join("  ");
        lines.push((labels, theme.accent()));
    }
    let description = task.description.trim();
    if !description.is_empty() {
        lines.push((String::new(), theme.text()));
        for part in wrap_parts(description, width) {
            lines.push((part, theme.text()));
        }
    }
    lines
}

fn place_cursor(frame: &mut Frame, app: &App, filter_area: Rect) {
    if app.overlay.is_some() || app.focus != Focus::Filter {
        return;
    }
    let room = filter_area.width.saturating_sub(4) as usize;
    let (_, col) = app.filter.window(room);
    let x = filter_area.x + 3 + col as u16;
    let max_x = filter_area.right().saturating_sub(1);
    if x < max_x {
        frame.set_cursor_position((x, filter_area.y));
    }
}

#[allow(clippy::too_many_arguments)]
fn action(
    buf: &mut ratatui::buffer::Buffer,
    x: u16,
    y: u16,
    max_x: u16,
    key: &str,
    label: &str,
    enabled: bool,
    danger: bool,
    theme: Theme,
) -> (Rect, u16) {
    let (key_style, label_style) = if !enabled {
        (theme.faint(), theme.faint())
    } else if danger {
        (
            theme.fg(theme.danger).add_modifier(Modifier::BOLD),
            theme.fg(theme.danger),
        )
    } else {
        (theme.bold_accent(), theme.text())
    };
    let key_rect = put(buf, x, y, max_x, key, key_style);
    if key_rect.width == 0 {
        return (
            Rect {
                x,
                y,
                width: 0,
                height: 1,
            },
            x,
        );
    }
    let label_rect = put(
        buf,
        key_rect.right().saturating_add(1),
        y,
        max_x,
        label,
        label_style,
    );
    let width = label_rect.right().saturating_sub(key_rect.x).max(1);
    (
        Rect {
            x: key_rect.x,
            y,
            width,
            height: 1,
        },
        label_rect.right(),
    )
}

fn with_bg(style: Style, bg: Option<ratatui::style::Color>) -> Style {
    match bg {
        Some(bg) => style.bg(bg),
        None => style,
    }
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    }
}

fn hline(buf: &mut ratatui::buffer::Buffer, x: u16, y: u16, width: u16, theme: Theme) {
    for col in x..x.saturating_add(width) {
        buf[(col, y)]
            .set_symbol("─")
            .set_style(theme.fg(theme.border));
    }
}

fn fill(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    bg: ratatui::style::Color,
    fg: ratatui::style::Color,
) {
    let style = Style::new().bg(bg).fg(fg);
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            buf[(x, y)].set_symbol(" ").set_style(style);
        }
    }
}

fn put(
    buf: &mut ratatui::buffer::Buffer,
    x: u16,
    y: u16,
    max_x: u16,
    text: &str,
    style: Style,
) -> Rect {
    if y >= buf.area.height || x >= max_x || x >= buf.area.width {
        return Rect {
            x,
            y,
            width: 0,
            height: 1,
        };
    }
    let max_x = max_x.min(buf.area.width);
    let fitted = fit(text, (max_x - x) as usize);
    let width = UnicodeWidthStr::width(fitted.as_str()) as u16;
    if width > 0 {
        buf.set_string(x, y, &fitted, style);
    }
    Rect {
        x,
        y,
        width,
        height: 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::todoist::{Due, Project, Task, User};
    use chrono::NaiveDate;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn task(id: &str, project: &str, content: &str, date: &str, text: &str) -> Task {
        Task {
            id: id.to_string(),
            content: content.to_string(),
            description: if id == "milk" {
                "oat".to_string()
            } else if id == "old" {
                "Coverage.\nCURRENT: keep".to_string()
            } else {
                String::new()
            },
            project_id: project.to_string(),
            section_id: None,
            parent_id: None,
            priority: if id == "old" { 4 } else { 1 },
            labels: Vec::new(),
            due: Some(Due {
                date: date.to_string(),
                string: text.to_string(),
                is_recurring: id == "old",
            }),
            child_order: 0,
            day_order: -1,
            note_count: 0,
            checked: false,
            is_deleted: false,
        }
    }

    #[allow(clippy::field_reassign_with_default)]
    fn preview() -> App {
        let mut app = App::default();
        app.now_override = Some(
            NaiveDate::from_ymd_opt(2026, 10, 16)
                .unwrap()
                .and_hms_opt(9, 0, 0)
                .unwrap(),
        );
        app.user = Some(User {
            full_name: "Sean".to_string(),
            email: "sean@example.com".to_string(),
            inbox_project_id: "in".to_string(),
        });
        app.projects = vec![
            Project {
                id: "in".to_string(),
                name: "Inbox".to_string(),
                parent_id: None,
                child_order: 0,
                color: "grey".to_string(),
                is_inbox_project: true,
                is_archived: false,
                is_deleted: false,
            },
            Project {
                id: "personal".to_string(),
                name: "Personal".to_string(),
                parent_id: None,
                child_order: 1,
                color: "blue".to_string(),
                is_inbox_project: false,
                is_archived: false,
                is_deleted: false,
            },
        ];
        app.tasks = vec![
            task("old", "in", "Call Mom", "2026-10-09", "Oct 9"),
            task("milk", "personal", "Buy milk", "2026-10-16", "today"),
        ];
        app.clamp_selection();
        app
    }

    fn render(app: &App) -> String {
        let backend = TestBackend::new(110, 32);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                draw(frame, app);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn today_view_shows_the_account_nav_and_grouped_tasks() {
        let app = preview();
        let text = render(&app);
        assert!(text.contains("Sean"), "{text}");
        assert!(text.contains("Inbox"), "{text}");
        assert!(text.contains("Today"), "{text}");
        assert!(text.contains("Upcoming"), "{text}");
        assert!(text.contains("My Projects"), "{text}");
        assert!(text.contains("Personal"), "{text}");
        assert!(text.contains("Overdue"), "{text}");
        assert!(text.contains("Call Mom"), "{text}");
        assert!(text.contains("Oct 16 · Today · Friday"), "{text}");
        assert!(text.contains("Buy milk"), "{text}");
        assert!(!text.contains("Add task"), "{text}");
        assert!(text.contains("p1 urgent"), "{text}");
        let overdue = text.find("Overdue").unwrap();
        let milk = text.find("Buy milk").unwrap();
        let detail = text.find("p1 urgent").unwrap();
        assert!(milk < detail, "{text}");
        let mom = text.find("Call Mom").unwrap();
        let heading = text.find("Oct 16 · Today · Friday").unwrap();
        assert!(overdue < mom && mom < heading && heading < milk, "{text}");
        let detail = &text[detail..];
        assert!(!detail.contains("Coverage.CURRENT"), "{detail}");
        let cover = detail.find("Coverage.").unwrap();
        let current = detail.find("CURRENT:").unwrap();
        let cover_line = detail[..cover].matches('\n').count();
        let current_line = detail[..current].matches('\n').count();
        assert!(cover_line < current_line, "{detail}");
    }

    #[test]
    fn a_short_list_leaves_the_rest_of_the_column_for_detail() {
        let rest = Rect {
            x: 0,
            y: 0,
            width: 40,
            height: 20,
        };
        let (list, rule, detail) = split_list_detail(rest, 4);
        let rule = rule.unwrap();
        assert_eq!(list.height, 4);
        assert_eq!(list.height + rule.height + detail.height, 20);
        assert!(detail.height * 100 / rest.height >= 30);
    }

    #[test]
    fn a_long_list_stops_at_seventy_percent_and_detail_keeps_thirty() {
        let rest = Rect {
            x: 0,
            y: 0,
            width: 40,
            height: 20,
        };
        let (list, rule, detail) = split_list_detail(rest, 100);
        let rule = rule.unwrap();
        assert!(list.height * 100 / rest.height <= 70);
        assert!(detail.height * 100 / rest.height >= 30);
        assert_eq!(list.height + rule.height + detail.height, 20);
    }

    #[test]
    fn the_add_modal_fits_its_text_and_stays_within_eighty_percent() {
        let roomy = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 40,
        };
        let fitted = add_modal(
            roomy,
            "Adds to Inbox unless the text names a #project",
            None,
            false,
        );
        assert!(fitted.height < 40 * 80 / 100);
        assert!(
            fitted.height < 16,
            "hugs the content, got {}",
            fitted.height
        );
        let cramped = Rect {
            x: 0,
            y: 0,
            width: 24,
            height: 20,
        };
        let capped = add_modal(cramped, &"word ".repeat(80), Some(&"no ".repeat(40)), false);
        assert_eq!(capped.height, 20 * 80 / 100);
    }

    #[test]
    fn scrolling_the_detail_moves_a_long_description() {
        let mut app = preview();
        app.tasks[0].description = (0..40)
            .map(|index| format!("LINE{index:02}"))
            .collect::<Vec<_>>()
            .join("\n");
        let top = detail_text(&render(&app));
        assert!(top.contains("LINE00"), "{top}");
        app.detail_scroll = 12;
        let scrolled = detail_text(&render(&app));
        assert!(!scrolled.contains("LINE00"), "{scrolled}");
        assert!(scrolled.contains("LINE08"), "{scrolled}");
    }

    fn detail_text(text: &str) -> String {
        let lines: Vec<&str> = text.lines().collect();
        lines
            .get(12..31)
            .map(|rows| rows.join("\n"))
            .unwrap_or_default()
    }
}
