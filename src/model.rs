use std::collections::{BTreeMap, HashSet};

use chrono::{Datelike, Days, NaiveDate, NaiveDateTime, NaiveTime, Timelike, Weekday};

use crate::text::{display_width, slice_cols};
use crate::todoist::{Due, Project, Task};

/// Days after today, through the seventh day. Today and overdue stay on Today.
pub const UPCOMING_DAYS: u64 = 7;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum View {
    Inbox,
    Today,
    Upcoming,
    Project(String),
}

impl View {
    pub fn eyebrow(&self) -> &'static str {
        match self {
            Self::Inbox => "inbox",
            Self::Today => "today",
            Self::Upcoming => "upcoming",
            Self::Project(_) => "project",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NavItem {
    Blank,
    Header,
    Entry(View),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavRow {
    pub item: NavItem,
    pub depth: usize,
    pub count: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderKind {
    Overdue,
    NoDate,
    Date(NaiveDate),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LineKind {
    Blank,
    Header(HeaderKind),
    TaskTop(usize),
    TaskBody(usize),
}

#[derive(Clone, Debug)]
pub struct Editor {
    pub value: String,
    pub cursor: usize,
}

impl Default for Editor {
    fn default() -> Self {
        Self::new("")
    }
}

impl Editor {
    pub fn new(value: impl Into<String>) -> Self {
        let value = value.into();
        let cursor = value.len();
        Self { value, cursor }
    }

    pub fn insert_str(&mut self, text: &str) {
        let room = 500usize.saturating_sub(self.value.chars().count());
        let clean: String = text
            .chars()
            .map(|ch| if ch == '\n' || ch == '\r' { ' ' } else { ch })
            .filter(|ch| !ch.is_control())
            .take(room)
            .collect();
        if clean.is_empty() {
            return;
        }
        self.cursor = self.cursor.min(self.value.len());
        self.value.insert_str(self.cursor, &clean);
        self.cursor += clean.len();
    }

    pub fn insert_char(&mut self, ch: char) {
        if ch.is_control() {
            return;
        }
        self.insert_str(&ch.to_string());
    }

    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let prev = prev_boundary(&self.value, self.cursor);
        self.value.replace_range(prev..self.cursor, "");
        self.cursor = prev;
    }

    pub fn delete_forward(&mut self) {
        if self.cursor >= self.value.len() {
            return;
        }
        let next = next_boundary(&self.value, self.cursor);
        self.value.replace_range(self.cursor..next, "");
    }

    pub fn move_left(&mut self) {
        self.cursor = prev_boundary(&self.value, self.cursor.min(self.value.len()));
    }

    pub fn move_right(&mut self) {
        self.cursor = next_boundary(&self.value, self.cursor.min(self.value.len()));
    }

    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor = self.value.len();
    }

    pub fn kill_to_start(&mut self) {
        let cursor = self.cursor.min(self.value.len());
        self.value.replace_range(0..cursor, "");
        self.cursor = 0;
    }

    pub fn delete_word_back(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let cursor = self.cursor.min(self.value.len());
        let before: Vec<(usize, char)> = self.value[..cursor].char_indices().collect();
        let mut end = cursor;
        let mut index = before.len();
        while index > 0 && before[index - 1].1.is_whitespace() {
            index -= 1;
            end = before[index].0;
        }
        if index == 0 {
            self.value.replace_range(0..cursor, "");
            self.cursor = 0;
            return;
        }
        let word = is_word(before[index - 1].1);
        while index > 0 {
            let ch = before[index - 1].1;
            if ch.is_whitespace() || is_word(ch) != word {
                break;
            }
            index -= 1;
            end = before[index].0;
        }
        self.value.replace_range(end..cursor, "");
        self.cursor = end;
    }

    pub fn window(&self, width: usize) -> (String, usize) {
        if width == 0 {
            return (String::new(), 0);
        }
        let cursor = self.cursor.min(self.value.len());
        let cursor_col = display_width(&self.value[..cursor]);
        let start = cursor_col.saturating_add(1).saturating_sub(width);
        let total = display_width(&self.value);
        let end = start.saturating_add(width).min(total);
        let visible = slice_cols(&self.value, start, end);
        (visible, cursor_col - start)
    }
}

fn is_word(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | '#')
}

fn prev_boundary(value: &str, cursor: usize) -> usize {
    let cursor = cursor.min(value.len());
    value[..cursor]
        .char_indices()
        .next_back()
        .map(|(index, _)| index)
        .unwrap_or(0)
}

fn next_boundary(value: &str, cursor: usize) -> usize {
    let cursor = cursor.min(value.len());
    value[cursor..]
        .chars()
        .next()
        .map(|ch| cursor + ch.len_utf8())
        .unwrap_or(cursor)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DueBucket {
    Overdue,
    Today,
    Later(NaiveDate),
    None,
}

pub struct DueText {
    pub label: String,
    pub overdue: bool,
}

pub fn inbox_id<'a>(projects: &'a [Project], user_inbox: &'a str) -> Option<&'a str> {
    if !user_inbox.is_empty() {
        return Some(user_inbox);
    }
    projects
        .iter()
        .find(|project| project.is_inbox_project)
        .map(|project| project.id.as_str())
        .or_else(|| {
            projects
                .iter()
                .find(|project| project.name.eq_ignore_ascii_case("inbox"))
                .map(|project| project.id.as_str())
        })
}

pub fn project_by_id<'a>(projects: &'a [Project], id: &str) -> Option<&'a Project> {
    projects.iter().find(|project| project.id == id)
}

pub fn ordered_projects<'a>(
    projects: &'a [Project],
    inbox: Option<&str>,
) -> Vec<(usize, &'a Project)> {
    let visible: Vec<&Project> = projects
        .iter()
        .filter(|project| project_visible(project, inbox))
        .collect();
    let ids: HashSet<&str> = visible.iter().map(|project| project.id.as_str()).collect();
    let mut children: BTreeMap<&str, Vec<&Project>> = BTreeMap::new();
    let mut roots = Vec::new();
    for project in &visible {
        match project.parent().filter(|parent| ids.contains(parent)) {
            Some(parent) => children.entry(parent).or_default().push(project),
            None => roots.push(*project),
        }
    }
    sort_projects(&mut roots);
    for kids in children.values_mut() {
        sort_projects(kids);
    }
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for root in roots {
        walk_project(root, 0, &children, &mut out, &mut seen);
    }
    out
}

fn project_visible(project: &Project, inbox: Option<&str>) -> bool {
    if project.is_archived || project.is_deleted || project.id.is_empty() {
        return false;
    }
    if project.is_inbox_project || inbox == Some(project.id.as_str()) {
        return false;
    }
    true
}

fn sort_projects(projects: &mut [&Project]) {
    projects.sort_by(|left, right| {
        left.child_order
            .cmp(&right.child_order)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
            .then_with(|| left.id.cmp(&right.id))
    });
}

fn walk_project<'a>(
    project: &'a Project,
    depth: usize,
    children: &BTreeMap<&str, Vec<&'a Project>>,
    out: &mut Vec<(usize, &'a Project)>,
    seen: &mut HashSet<String>,
) {
    if depth > 6 || !seen.insert(project.id.clone()) {
        return;
    }
    out.push((depth, project));
    if let Some(kids) = children.get(project.id.as_str()) {
        for child in kids {
            walk_project(child, depth + 1, children, out, seen);
        }
    }
}

pub fn nav_rows(
    projects: &[Project],
    tasks: &[Task],
    inbox: Option<&str>,
    today: NaiveDate,
) -> Vec<NavRow> {
    let mut rows = vec![
        entry(View::Inbox, count_view(&View::Inbox, tasks, inbox, today)),
        entry(View::Today, count_view(&View::Today, tasks, inbox, today)),
        entry(
            View::Upcoming,
            count_view(&View::Upcoming, tasks, inbox, today),
        ),
        NavRow {
            item: NavItem::Blank,
            depth: 0,
            count: None,
        },
        NavRow {
            item: NavItem::Header,
            depth: 0,
            count: None,
        },
    ];
    for (depth, project) in ordered_projects(projects, inbox) {
        rows.push(NavRow {
            item: NavItem::Entry(View::Project(project.id.clone())),
            depth,
            count: Some(count_view(
                &View::Project(project.id.clone()),
                tasks,
                inbox,
                today,
            )),
        });
    }
    rows
}

fn entry(view: View, count: usize) -> NavRow {
    NavRow {
        item: NavItem::Entry(view),
        depth: 0,
        count: Some(count),
    }
}

fn count_view(view: &View, tasks: &[Task], inbox: Option<&str>, today: NaiveDate) -> usize {
    visible_tasks(tasks, view, inbox, today, "", &[]).len()
}

pub fn entry_label(view: &View, projects: &[Project]) -> String {
    match view {
        View::Inbox => "Inbox".to_string(),
        View::Today => "Today".to_string(),
        View::Upcoming => "Upcoming".to_string(),
        View::Project(id) => project_by_id(projects, id)
            .map(|project| project.name.clone())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "Project".to_string()),
    }
}

pub fn visible_tasks(
    tasks: &[Task],
    view: &View,
    inbox: Option<&str>,
    today: NaiveDate,
    query: &str,
    projects: &[Project],
) -> Vec<usize> {
    let mut indices: Vec<usize> = tasks
        .iter()
        .enumerate()
        .filter(|(_, task)| task.active() && in_view(task, view, inbox, today))
        .filter(|(_, task)| matches_query(task, projects, query))
        .map(|(index, _)| index)
        .collect();
    let by_day = matches!(view, View::Today | View::Upcoming);
    indices.sort_by(|&left, &right| compare_tasks(&tasks[left], &tasks[right], by_day));
    indices
}

fn in_view(task: &Task, view: &View, inbox: Option<&str>, today: NaiveDate) -> bool {
    match view {
        View::Inbox => inbox == Some(task.project_id.as_str()),
        View::Project(id) => task.project_id == *id,
        View::Today => matches!(
            due_bucket(task.due.as_ref(), today),
            DueBucket::Overdue | DueBucket::Today
        ),
        View::Upcoming => match due_bucket(task.due.as_ref(), today) {
            DueBucket::Later(day) => within_upcoming(day, today),
            _ => false,
        },
    }
}

fn within_upcoming(day: NaiveDate, today: NaiveDate) -> bool {
    let Some(end) = today.checked_add_days(Days::new(UPCOMING_DAYS)) else {
        return false;
    };
    day > today && day <= end
}

fn compare_tasks(left: &Task, right: &Task, by_day: bool) -> std::cmp::Ordering {
    let primary = if by_day {
        due_sort_key(left)
            .cmp(&due_sort_key(right))
            .then(day_key(left).cmp(&day_key(right)))
            .then(left.child_order.cmp(&right.child_order))
    } else {
        left.child_order.cmp(&right.child_order)
    };
    primary
        .then(right.priority.cmp(&left.priority))
        .then_with(|| {
            left.content
                .to_lowercase()
                .cmp(&right.content.to_lowercase())
        })
        .then(left.id.cmp(&right.id))
}

fn due_sort_key(task: &Task) -> NaiveDate {
    task.due
        .as_ref()
        .and_then(calendar_day)
        .unwrap_or(NaiveDate::MAX)
}

fn day_key(task: &Task) -> i64 {
    if task.day_order < 0 {
        i64::MAX
    } else {
        task.day_order
    }
}

pub fn matches_query(task: &Task, projects: &[Project], query: &str) -> bool {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return true;
    }
    let project = project_by_id(projects, &task.project_id)
        .map(|project| project.name.as_str())
        .unwrap_or("");
    task.content.to_lowercase().contains(&query)
        || task.description.to_lowercase().contains(&query)
        || project.to_lowercase().contains(&query)
        || task
            .labels
            .iter()
            .any(|label| label.to_lowercase().contains(&query))
        || task.due.as_ref().is_some_and(|due| {
            due.string.to_lowercase().contains(&query) || due.date.to_lowercase().contains(&query)
        })
}

pub fn due_bucket(due: Option<&Due>, today: NaiveDate) -> DueBucket {
    let Some(due) = due else {
        return DueBucket::None;
    };
    let Some(day) = calendar_day(due) else {
        return DueBucket::None;
    };
    if day < today {
        DueBucket::Overdue
    } else if day == today {
        DueBucket::Today
    } else {
        DueBucket::Later(day)
    }
}

pub fn calendar_day(due: &Due) -> Option<NaiveDate> {
    let raw = due.date.trim();
    if raw.is_empty() {
        return None;
    }
    let Some((day, time)) = raw.split_once('T') else {
        return NaiveDate::parse_from_str(&raw[..raw.len().min(10)], "%Y-%m-%d").ok();
    };
    if is_zoned(time)
        && let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(raw)
    {
        return Some(parsed.with_timezone(&chrono::Local).date_naive());
    }
    NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()
}

fn is_zoned(time: &str) -> bool {
    time.ends_with('Z') || time.contains('+') || time[1..].contains('-')
}

pub fn due_text(due: &Due, now: NaiveDateTime) -> DueText {
    let overdue = is_overdue(due, now);
    let mut label = if due.string.trim().is_empty() {
        fallback_due(due, now.date())
    } else {
        due.string.trim().to_string()
    };
    if due.is_recurring && !label.contains('↻') {
        label.push_str(" ↻");
    }
    DueText { label, overdue }
}

pub fn is_overdue(due: &Due, now: NaiveDateTime) -> bool {
    let Some(day) = calendar_day(due) else {
        return false;
    };
    if day < now.date() {
        return true;
    }
    if day > now.date() {
        return false;
    }
    clock_time(due).is_some_and(|time| time < now.time())
}

fn fallback_due(due: &Due, today: NaiveDate) -> String {
    let Some(day) = calendar_day(due) else {
        return due.date.clone();
    };
    let name = if day == today {
        "Today".to_string()
    } else if today.pred_opt() == Some(day) {
        "Yesterday".to_string()
    } else if today.succ_opt() == Some(day) {
        "Tomorrow".to_string()
    } else {
        format_month_day(day)
    };
    match clock_time(due) {
        Some(time) => format!("{name} {}", format_clock(time)),
        None => name,
    }
}

fn clock_time(due: &Due) -> Option<NaiveTime> {
    let raw = due.date.trim();
    let (_, time) = raw.split_once('T')?;
    if is_zoned(time) {
        let parsed = chrono::DateTime::parse_from_rfc3339(raw).ok()?;
        return Some(parsed.with_timezone(&chrono::Local).time());
    }
    parse_hms(time)
}

fn parse_hms(time: &str) -> Option<NaiveTime> {
    let head = time.split(['Z', '+', '-']).next().unwrap_or(time);
    if let Ok(parsed) = NaiveTime::parse_from_str(head, "%H:%M:%S") {
        return Some(parsed);
    }
    NaiveTime::parse_from_str(head, "%H:%M").ok()
}

fn format_clock(time: NaiveTime) -> String {
    let hour = time.hour();
    let suffix = if hour < 12 { "AM" } else { "PM" };
    let hour12 = match hour % 12 {
        0 => 12,
        hour => hour,
    };
    if time.minute() == 0 {
        format!("{hour12} {suffix}")
    } else {
        format!("{hour12}:{:02} {suffix}", time.minute())
    }
}

pub fn header_text(kind: HeaderKind, today: NaiveDate) -> String {
    match kind {
        HeaderKind::Overdue => "Overdue".to_string(),
        HeaderKind::NoDate => "No date".to_string(),
        HeaderKind::Date(day) => date_heading(day, today),
    }
}

pub fn date_heading(day: NaiveDate, today: NaiveDate) -> String {
    let date = format_month_day(day);
    let weekday = weekday_name(day.weekday());
    if day == today {
        format!("{date} · Today · {weekday}")
    } else {
        format!("{date} · {weekday}")
    }
}

fn format_month_day(day: NaiveDate) -> String {
    format!("{} {}", month_name(day.month()), day.day())
}

fn month_name(month: u32) -> &'static str {
    match month {
        1 => "Jan",
        2 => "Feb",
        3 => "Mar",
        4 => "Apr",
        5 => "May",
        6 => "Jun",
        7 => "Jul",
        8 => "Aug",
        9 => "Sep",
        10 => "Oct",
        11 => "Nov",
        _ => "Dec",
    }
}

fn weekday_name(day: Weekday) -> &'static str {
    match day {
        Weekday::Mon => "Monday",
        Weekday::Tue => "Tuesday",
        Weekday::Wed => "Wednesday",
        Weekday::Thu => "Thursday",
        Weekday::Fri => "Friday",
        Weekday::Sat => "Saturday",
        Weekday::Sun => "Sunday",
    }
}

pub fn layout_lines(
    tasks: &[Task],
    indices: &[usize],
    view: &View,
    today: NaiveDate,
) -> Vec<LineKind> {
    let mut lines = Vec::new();
    let mut groups: Vec<(HeaderKind, Vec<usize>)> = Vec::new();
    match view {
        View::Today => {
            push_group(
                &mut groups,
                HeaderKind::Overdue,
                indices
                    .iter()
                    .copied()
                    .filter(|&index| {
                        due_bucket(tasks[index].due.as_ref(), today) == DueBucket::Overdue
                    })
                    .collect(),
            );
            push_group(
                &mut groups,
                HeaderKind::Date(today),
                indices
                    .iter()
                    .copied()
                    .filter(|&index| {
                        due_bucket(tasks[index].due.as_ref(), today) == DueBucket::Today
                    })
                    .collect(),
            );
        }
        View::Upcoming => {
            let mut days: BTreeMap<NaiveDate, Vec<usize>> = BTreeMap::new();
            for index in indices.iter().copied() {
                if let DueBucket::Later(day) = due_bucket(tasks[index].due.as_ref(), today) {
                    days.entry(day).or_default().push(index);
                }
            }
            for (day, members) in days {
                push_group(&mut groups, HeaderKind::Date(day), members);
            }
        }
        View::Inbox | View::Project(_) => {
            let mut overdue = Vec::new();
            let mut due_today = Vec::new();
            let mut later: BTreeMap<NaiveDate, Vec<usize>> = BTreeMap::new();
            let mut undated = Vec::new();
            for index in indices.iter().copied() {
                match due_bucket(tasks[index].due.as_ref(), today) {
                    DueBucket::Overdue => overdue.push(index),
                    DueBucket::Today => due_today.push(index),
                    DueBucket::Later(day) => later.entry(day).or_default().push(index),
                    DueBucket::None => undated.push(index),
                }
            }
            let only_undated = overdue.is_empty() && due_today.is_empty() && later.is_empty();
            push_group(&mut groups, HeaderKind::Overdue, overdue);
            push_group(&mut groups, HeaderKind::Date(today), due_today);
            for (day, members) in later {
                push_group(&mut groups, HeaderKind::Date(day), members);
            }
            if !only_undated {
                push_group(&mut groups, HeaderKind::NoDate, undated);
            } else if !undated.is_empty() {
                groups.push((HeaderKind::NoDate, undated));
            }
        }
    }
    let show_undated_header = !matches!(view, View::Inbox | View::Project(_))
        || groups.iter().any(|(kind, _)| *kind != HeaderKind::NoDate);
    for (offset, (kind, members)) in groups.into_iter().enumerate() {
        if offset > 0 {
            lines.push(LineKind::Blank);
        }
        if show_undated_header || kind != HeaderKind::NoDate {
            lines.push(LineKind::Header(kind));
        }
        for index in members {
            lines.push(LineKind::TaskTop(index));
            if task_has_body(&tasks[index], view) {
                lines.push(LineKind::TaskBody(index));
            }
        }
    }
    lines
}

fn push_group(groups: &mut Vec<(HeaderKind, Vec<usize>)>, kind: HeaderKind, members: Vec<usize>) {
    if !members.is_empty() {
        groups.push((kind, members));
    }
}

pub fn task_has_body(task: &Task, view: &View) -> bool {
    task.due.is_some()
        || !task.description.trim().is_empty()
        || !task.labels.is_empty()
        || task.note_count > 0
        || matches!(view, View::Today | View::Upcoming)
}

pub fn priority_name(priority: i32) -> &'static str {
    match priority {
        4 => "p1 urgent",
        3 => "p2 high",
        2 => "p3 medium",
        _ => "p4",
    }
}

/// Append `#Project` when quick add did not already name one.
pub fn quick_add_text(typed: &str, project: Option<&str>) -> String {
    let typed = typed.trim();
    let Some(project) = project.map(str::trim).filter(|name| !name.is_empty()) else {
        return typed.to_string();
    };
    if typed.contains('#') {
        return typed.to_string();
    }
    let escaped = project.replace('\\', "\\\\").replace(' ', "\\ ");
    format!("{typed} #{escaped}")
}

pub fn empty_message(view: &View, filtering: bool) -> &'static str {
    if filtering {
        return "No matches";
    }
    match view {
        View::Today => "Nothing due today",
        View::Upcoming => "Nothing due in the next 7 days",
        View::Inbox => "Inbox is empty",
        View::Project(_) => "No tasks",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::todoist::Task;

    fn day(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap()
    }

    fn at(date: NaiveDate, hour: u32) -> NaiveDateTime {
        date.and_hms_opt(hour, 0, 0).unwrap()
    }

    fn task(id: &str, project: &str, content: &str, priority: i32) -> Task {
        Task {
            id: id.to_string(),
            content: content.to_string(),
            description: String::new(),
            project_id: project.to_string(),
            section_id: None,
            parent_id: None,
            priority,
            labels: Vec::new(),
            due: None,
            child_order: 0,
            day_order: -1,
            note_count: 0,
            checked: false,
            is_deleted: false,
        }
    }

    fn with_due(mut task: Task, date: &str, text: &str) -> Task {
        task.due = Some(Due {
            date: date.to_string(),
            string: text.to_string(),
            is_recurring: false,
        });
        task
    }

    fn project(id: &str, name: &str, order: i64, parent: Option<&str>) -> Project {
        Project {
            id: id.to_string(),
            name: name.to_string(),
            parent_id: parent.map(str::to_string),
            child_order: order,
            color: "grey".to_string(),
            is_inbox_project: id == "in",
            is_archived: false,
            is_deleted: false,
        }
    }

    fn titles(lines: &[LineKind], today: NaiveDate) -> Vec<String> {
        lines
            .iter()
            .filter_map(|line| match line {
                LineKind::Header(kind) => Some(header_text(*kind, today)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn today_groups_overdue_before_the_date_heading() {
        let today = day(2026, 10, 16);
        let tasks = vec![
            with_due(task("now", "p", "Buy milk", 1), "2026-10-16", "today"),
            with_due(task("old", "in", "Call Mom", 4), "2026-10-09", "Oct 9"),
            with_due(task("later", "p", "Later", 1), "2026-10-20", "Oct 20"),
        ];
        let indices = visible_tasks(&tasks, &View::Today, Some("in"), today, "", &[]);
        let lines = layout_lines(&tasks, &indices, &View::Today, today);
        assert_eq!(
            titles(&lines, today),
            vec!["Overdue".to_string(), "Oct 16 · Today · Friday".to_string()]
        );
        let order: Vec<_> = lines
            .iter()
            .filter_map(|line| match line {
                LineKind::TaskTop(index) => Some(tasks[*index].content.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(order, vec!["Call Mom", "Buy milk"]);
    }

    #[test]
    fn upcoming_is_the_seven_days_after_today() {
        let today = day(2026, 10, 16);
        let tasks = vec![
            with_due(task("old", "p", "Old", 1), "2026-10-15", ""),
            with_due(task("now", "p", "Now", 1), "2026-10-16", ""),
            with_due(task("soon", "p", "Soon", 1), "2026-10-17", ""),
            with_due(task("edge", "p", "Edge", 1), "2026-10-23", ""),
            with_due(task("far", "p", "Far", 1), "2026-10-24", ""),
        ];
        let indices = visible_tasks(&tasks, &View::Upcoming, None, today, "", &[]);
        let names: Vec<_> = indices
            .iter()
            .map(|index| tasks[*index].content.as_str())
            .collect();
        assert_eq!(names, vec!["Soon", "Edge"]);
    }

    #[test]
    fn inbox_keeps_only_that_project_and_skips_a_lone_no_date_header() {
        let today = day(2026, 10, 16);
        let tasks = vec![
            task("a", "in", "File mail", 1),
            task("b", "other", "Not inbox", 1),
        ];
        let indices = visible_tasks(&tasks, &View::Inbox, Some("in"), today, "", &[]);
        let lines = layout_lines(&tasks, &indices, &View::Inbox, today);
        assert!(titles(&lines, today).is_empty());
        assert!(matches!(lines[0], LineKind::TaskTop(0)));
    }

    #[test]
    fn projects_nest_under_parents_and_leave_out_the_inbox() {
        let projects = vec![
            project("in", "Inbox", 0, None),
            project("child", "Personal Health", 2, Some("parent")),
            project("parent", "Personal", 1, None),
            project("later", "someday", 3, None),
        ];
        let ordered = ordered_projects(&projects, Some("in"));
        let names: Vec<_> = ordered
            .iter()
            .map(|(depth, project)| (*depth, project.name.as_str()))
            .collect();
        assert_eq!(
            names,
            vec![(0, "Personal"), (1, "Personal Health"), (0, "someday")]
        );
    }

    #[test]
    fn a_date_only_task_today_is_not_overdue_and_yesterday_is() {
        let now = at(day(2026, 10, 16), 9);
        let today = Due {
            date: "2026-10-16".to_string(),
            string: String::new(),
            is_recurring: false,
        };
        let yesterday = Due {
            date: "2026-10-15T09:00:00".to_string(),
            string: "Yesterday 9 AM".to_string(),
            is_recurring: true,
        };
        assert!(!is_overdue(&today, now));
        assert!(is_overdue(&yesterday, now));
        assert_eq!(due_text(&yesterday, now).label, "Yesterday 9 AM ↻");
        assert_eq!(due_text(&today, now).label, "Today");
    }

    #[test]
    fn quick_add_names_the_open_project_unless_the_text_already_does() {
        assert_eq!(
            quick_add_text("Order filters", Some("Personal Health")),
            r"Order filters #Personal\ Health"
        );
        assert_eq!(
            quick_add_text("Order filters #Garage", Some("Personal")),
            "Order filters #Garage"
        );
        assert_eq!(quick_add_text("Call Mom", None), "Call Mom");
    }

    #[test]
    fn filter_matches_content_project_and_label() {
        let projects = vec![project("p", "Personal Health", 0, None)];
        let mut task = task("a", "p", "Schedule labs", 2);
        task.labels = vec!["health".to_string()];
        assert!(matches_query(&task, &projects, "labs"));
        assert!(matches_query(&task, &projects, "health"));
        assert!(!matches_query(&task, &projects, "garage"));
    }
}
