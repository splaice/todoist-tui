use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use chrono::{Local, NaiveDate, NaiveDateTime};
use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

use crate::model::{
    Editor, LineKind, NavItem, NavRow, View, entry_label, layout_lines, nav_rows, project_by_id,
    quick_add_text, visible_tasks,
};
use crate::screen::{Screen, TaskHit, contains, hit};
use crate::todoist::{Client, Project, SyncData, Task, User};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Nav,
    Tasks,
    Detail,
    Filter,
}

pub enum Overlay {
    Help,
    Add(AddForm),
    Due(DueForm),
    Delete(DeleteForm),
}

pub struct AddForm {
    pub text: Editor,
    pub hint: String,
    pub error: Option<String>,
    pub busy: bool,
    pub generation: u64,
}

pub struct DueForm {
    pub task_id: String,
    pub content: String,
    pub due: Editor,
    pub error: Option<String>,
    pub busy: bool,
    pub generation: u64,
}

pub struct DeleteForm {
    pub task_id: String,
    pub content: String,
    pub error: Option<String>,
    pub busy: bool,
    pub generation: u64,
}

#[derive(Debug)]
pub enum Job {
    Sync {
        generation: u64,
    },
    QuickAdd {
        generation: u64,
        text: String,
    },
    Close {
        id: String,
    },
    Delete {
        generation: u64,
        id: String,
    },
    Reschedule {
        generation: u64,
        id: String,
        due: String,
    },
}

pub enum Msg {
    Synced {
        generation: u64,
        result: Result<SyncData, String>,
    },
    Added {
        generation: u64,
        result: Result<Option<Task>, String>,
    },
    Closed {
        id: String,
        result: Result<(), String>,
    },
    Deleted {
        generation: u64,
        id: String,
        result: Result<(), String>,
    },
    Rescheduled {
        generation: u64,
        result: Result<Task, String>,
    },
}

pub enum Effect {
    None,
    Quit,
    Jobs(Vec<Job>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Err,
}

pub struct Toast {
    pub text: String,
    pub kind: ToastKind,
    pub until: Instant,
}

pub struct App {
    pub client: Option<Client>,
    pub fatal: Option<String>,
    pub user: Option<User>,
    pub projects: Vec<Project>,
    pub tasks: Vec<Task>,
    pub loading: bool,
    pub load_error: Option<String>,
    pub focus: Focus,
    pub view: View,
    pub filter: Editor,
    pub selected_id: Option<String>,
    pub nav_offset: usize,
    pub nav_height: usize,
    pub nav_len: usize,
    pub task_scroll: usize,
    pub task_height: usize,
    pub task_len: usize,
    pub detail_rows: usize,
    pub detail_height: usize,
    pub detail_scroll: usize,
    pub overlay: Option<Overlay>,
    pub toast: Option<Toast>,
    pub closing: HashSet<String>,
    pub screen: Screen,
    pub(crate) now_override: Option<NaiveDateTime>,
    selections: HashMap<View, String>,
    sync_generation: u64,
    add_generation: u64,
    due_generation: u64,
    delete_generation: u64,
    mutations: u32,
    sync_deferred: bool,
}

impl Default for App {
    fn default() -> Self {
        Self {
            client: None,
            fatal: None,
            user: None,
            projects: Vec::new(),
            tasks: Vec::new(),
            loading: false,
            load_error: None,
            focus: Focus::Tasks,
            view: View::Today,
            filter: Editor::new(""),
            selected_id: None,
            nav_offset: 0,
            nav_height: 0,
            nav_len: 0,
            task_scroll: 0,
            task_height: 0,
            task_len: 0,
            detail_rows: 0,
            detail_height: 0,
            detail_scroll: 0,
            overlay: None,
            toast: None,
            closing: HashSet::new(),
            screen: Screen::default(),
            now_override: None,
            selections: HashMap::new(),
            sync_generation: 0,
            add_generation: 0,
            due_generation: 0,
            delete_generation: 0,
            mutations: 0,
            sync_deferred: false,
        }
    }
}

impl App {
    pub fn boot() -> (Self, Vec<Job>) {
        let mut app = Self::default();
        match Client::from_env() {
            Ok(client) => {
                app.client = Some(client);
                let job = app.request_sync();
                (app, vec![job])
            }
            Err(err) => {
                app.fatal = Some(err);
                (app, Vec::new())
            }
        }
    }

    pub fn adopt(&mut self, screen: Screen) {
        self.nav_height = screen.nav_height;
        self.nav_offset = screen.nav_offset;
        self.nav_len = screen.nav_len;
        self.task_height = screen.task_height;
        self.task_scroll = screen.task_scroll;
        self.task_len = screen.task_len;
        self.detail_rows = screen.detail_rows;
        self.detail_height = screen.detail_height;
        let max = self.detail_rows.saturating_sub(self.detail_height);
        self.detail_scroll = self.detail_scroll.min(max);
        self.screen = screen;
    }

    pub fn poll_timeout(&self) -> Duration {
        let idle = Duration::from_millis(50);
        match &self.toast {
            Some(toast) => {
                let now = Instant::now();
                if toast.until <= now {
                    Duration::from_millis(1)
                } else {
                    (toast.until - now).min(idle)
                }
            }
            None => idle,
        }
    }

    pub fn tick(&mut self) {
        if self
            .toast
            .as_ref()
            .is_some_and(|toast| toast.until <= Instant::now())
        {
            self.toast = None;
        }
    }

    pub fn toast(&mut self, text: impl Into<String>, kind: ToastKind) {
        let millis = match kind {
            ToastKind::Info => 1600,
            ToastKind::Err => 4200,
        };
        self.toast = Some(Toast {
            text: text.into(),
            kind,
            until: Instant::now() + Duration::from_millis(millis),
        });
    }

    pub fn now(&self) -> NaiveDateTime {
        self.now_override
            .unwrap_or_else(|| Local::now().naive_local())
    }

    pub fn today(&self) -> NaiveDate {
        self.now().date()
    }

    pub fn account_label(&self) -> String {
        self.user
            .as_ref()
            .map(User::label)
            .unwrap_or_else(|| "Todoist".to_string())
    }

    pub fn inbox_id(&self) -> Option<&str> {
        let stored = self
            .user
            .as_ref()
            .map(|user| user.inbox_project_id.as_str())
            .unwrap_or("");
        crate::model::inbox_id(&self.projects, stored)
    }

    pub fn navigation(&self) -> Vec<NavRow> {
        nav_rows(&self.projects, &self.tasks, self.inbox_id(), self.today())
    }

    pub fn filtered_indices(&self) -> Vec<usize> {
        visible_tasks(
            &self.tasks,
            &self.view,
            self.inbox_id(),
            self.today(),
            self.filter.value.trim(),
            &self.projects,
        )
    }

    /// Task indexes in the order they are drawn, top to bottom.
    pub fn ordered_tasks(&self) -> Vec<usize> {
        self.line_layout()
            .into_iter()
            .filter_map(|line| match line {
                LineKind::TaskTop(index) => Some(index),
                _ => None,
            })
            .collect()
    }

    pub fn line_layout(&self) -> Vec<LineKind> {
        layout_lines(
            &self.tasks,
            &self.filtered_indices(),
            &self.view,
            self.today(),
        )
    }

    pub fn view_title(&self) -> String {
        entry_label(&self.view, &self.projects)
    }

    pub fn view_count(&self) -> usize {
        visible_tasks(
            &self.tasks,
            &self.view,
            self.inbox_id(),
            self.today(),
            "",
            &self.projects,
        )
        .len()
    }

    pub fn shown_count(&self) -> usize {
        self.filtered_indices().len()
    }

    pub fn selected_task(&self) -> Option<&Task> {
        let id = self.selected_id.as_ref()?;
        self.tasks.iter().find(|task| &task.id == id)
    }

    pub fn project_named(&self, id: &str) -> Option<&Project> {
        project_by_id(&self.projects, id)
    }

    pub fn saving(&self) -> bool {
        self.mutations > 0
    }

    pub fn on_key(&mut self, key: KeyEvent) -> Effect {
        if key.kind == KeyEventKind::Release {
            return Effect::None;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Effect::Quit;
        }
        if self.fatal.is_some() {
            return if key.code == KeyCode::Char('q') {
                Effect::Quit
            } else {
                Effect::None
            };
        }
        if self.overlay.is_some() {
            return self.overlay_key(key);
        }
        if self.focus == Focus::Filter {
            return self.filter_key(key);
        }
        if let Some(effect) = self.global_key(&key) {
            return effect;
        }
        match self.focus {
            Focus::Nav => self.nav_key(key),
            Focus::Tasks => self.task_key(key),
            Focus::Detail => self.detail_key(key),
            Focus::Filter => Effect::None,
        }
    }

    pub fn on_paste(&mut self, text: String) -> Effect {
        if let Some(editor) = self.focused_editor() {
            editor.insert_str(&text);
        }
        if self.focus == Focus::Filter && self.overlay.is_none() {
            self.clamp_selection();
            self.task_scroll = 0;
            self.reveal_task();
        }
        Effect::None
    }

    pub fn on_mouse(&mut self, event: MouseEvent) -> Effect {
        let (x, y) = (event.column, event.row);
        match event.kind {
            MouseEventKind::Down(MouseButton::Left) => self.click(x, y),
            MouseEventKind::ScrollUp => self.scroll_at(x, y, -1),
            MouseEventKind::ScrollDown => self.scroll_at(x, y, 1),
            _ => Effect::None,
        }
    }

    pub fn on_msg(&mut self, msg: Msg) -> Vec<Job> {
        match msg {
            Msg::Synced { generation, result } => self.on_synced(generation, result),
            Msg::Added { generation, result } => self.on_added(generation, result),
            Msg::Closed { id, result } => self.on_closed(id, result),
            Msg::Deleted {
                generation,
                id,
                result,
            } => self.on_deleted(generation, id, result),
            Msg::Rescheduled { generation, result } => self.on_rescheduled(generation, result),
        }
    }

    fn on_synced(&mut self, generation: u64, result: Result<SyncData, String>) -> Vec<Job> {
        if generation != self.sync_generation {
            return Vec::new();
        }
        if self.mutations > 0 {
            self.sync_deferred = true;
            self.loading = false;
            return Vec::new();
        }
        self.loading = false;
        match result {
            Ok(data) => {
                self.load_error = None;
                self.user = Some(data.user);
                self.projects = data.projects;
                self.tasks = data.tasks;
                self.clamp_selection();
                self.reveal_task();
                self.reveal_nav();
            }
            Err(err) => {
                if self.tasks.is_empty() {
                    self.load_error = Some(err);
                } else {
                    self.toast(err, ToastKind::Err);
                }
            }
        }
        Vec::new()
    }

    fn on_added(&mut self, generation: u64, result: Result<Option<Task>, String>) -> Vec<Job> {
        self.mutations = self.mutations.saturating_sub(1);
        match result {
            Ok(task) => {
                let id = task.as_ref().map(|task| task.id.clone());
                if let Some(task) = task {
                    self.replace_task(task);
                }
                if generation == self.add_generation {
                    self.overlay = None;
                    if let Some(id) = id.clone() {
                        self.select(Some(id));
                    }
                }
                self.clamp_selection();
                if let Some(id) = &id
                    && self
                        .filtered_indices()
                        .iter()
                        .any(|&index| self.tasks[index].id == *id)
                {
                    self.select(Some(id.clone()));
                    self.reveal_task();
                }
                self.toast("Task added", ToastKind::Info);
                if id.is_none() {
                    self.sync_deferred = false;
                    return vec![self.request_sync()];
                }
                self.finish_mutation()
            }
            Err(err) => {
                if generation == self.add_generation
                    && let Some(Overlay::Add(form)) = &mut self.overlay
                {
                    form.busy = false;
                    form.error = Some(err);
                } else {
                    self.toast(err, ToastKind::Err);
                }
                self.finish_mutation()
            }
        }
    }

    fn on_closed(&mut self, id: String, result: Result<(), String>) -> Vec<Job> {
        self.closing.remove(&id);
        self.mutations = self.mutations.saturating_sub(1);
        match result {
            Ok(()) => {
                self.remove_task(&id);
                self.toast("Completed", ToastKind::Info);
            }
            Err(err) => self.toast(err, ToastKind::Err),
        }
        self.finish_mutation()
    }

    fn on_deleted(&mut self, generation: u64, id: String, result: Result<(), String>) -> Vec<Job> {
        self.mutations = self.mutations.saturating_sub(1);
        match result {
            Ok(()) => {
                self.remove_task(&id);
                if generation == self.delete_generation {
                    self.overlay = None;
                }
                self.toast("Deleted", ToastKind::Info);
            }
            Err(err) => {
                if generation == self.delete_generation
                    && let Some(Overlay::Delete(form)) = &mut self.overlay
                {
                    form.busy = false;
                    form.error = Some(err);
                } else {
                    self.toast(err, ToastKind::Err);
                }
            }
        }
        self.finish_mutation()
    }

    fn on_rescheduled(&mut self, generation: u64, result: Result<Task, String>) -> Vec<Job> {
        self.mutations = self.mutations.saturating_sub(1);
        match result {
            Ok(task) => {
                self.replace_task(task);
                if generation == self.due_generation {
                    self.overlay = None;
                }
                self.clamp_selection();
                self.reveal_task();
                self.toast("Rescheduled", ToastKind::Info);
            }
            Err(err) => {
                if generation == self.due_generation
                    && let Some(Overlay::Due(form)) = &mut self.overlay
                {
                    form.busy = false;
                    form.error = Some(err);
                } else {
                    self.toast(err, ToastKind::Err);
                }
            }
        }
        self.finish_mutation()
    }

    fn request_sync(&mut self) -> Job {
        self.sync_generation += 1;
        self.loading = true;
        Job::Sync {
            generation: self.sync_generation,
        }
    }

    fn refresh(&mut self) -> Effect {
        if self.client.is_none() {
            return Effect::None;
        }
        if self.mutations > 0 {
            self.sync_deferred = true;
            Effect::None
        } else {
            Effect::Jobs(vec![self.request_sync()])
        }
    }

    fn begin_mutation(&mut self) {
        self.mutations += 1;
    }

    fn finish_mutation(&mut self) -> Vec<Job> {
        if self.mutations == 0 && self.sync_deferred {
            self.sync_deferred = false;
            vec![self.request_sync()]
        } else {
            Vec::new()
        }
    }

    fn replace_task(&mut self, task: Task) {
        if let Some(slot) = self.tasks.iter_mut().find(|item| item.id == task.id) {
            *slot = task;
        } else {
            self.tasks.push(task);
        }
    }

    fn remove_task(&mut self, id: &str) {
        let indices = self.ordered_tasks();
        let pos = indices.iter().position(|&index| self.tasks[index].id == id);
        let next = if self.selected_id.as_deref() == Some(id) {
            pos.and_then(|pos| {
                indices
                    .get(pos + 1)
                    .or(pos.checked_sub(1).and_then(|index| indices.get(index)))
            })
            .and_then(|&index| {
                let next_id = self.tasks[index].id.clone();
                (next_id != id).then_some(next_id)
            })
        } else {
            None
        };
        let replacing = self.selected_id.as_deref() == Some(id);
        self.tasks.retain(|task| task.id != id);
        if replacing {
            self.select(next);
        }
        self.selections.retain(|_, saved| saved != id);
    }

    fn global_key(&mut self, key: &KeyEvent) -> Option<Effect> {
        if key.modifiers.contains(KeyModifiers::CONTROL)
            || key.modifiers.contains(KeyModifiers::ALT)
        {
            return None;
        }
        match key.code {
            KeyCode::Char('q') => Some(Effect::Quit),
            KeyCode::Char('?') => {
                self.overlay = Some(Overlay::Help);
                Some(Effect::None)
            }
            KeyCode::Char('/') => {
                self.focus = Focus::Filter;
                Some(Effect::None)
            }
            KeyCode::Char('n') => {
                self.open_add();
                Some(Effect::None)
            }
            KeyCode::Char('r') => Some(self.refresh()),
            KeyCode::Char('x') | KeyCode::Char(' ') if self.selected_task().is_some() => {
                Some(self.close_selected())
            }
            KeyCode::Char('e') if self.selected_task().is_some() => {
                self.open_due();
                Some(Effect::None)
            }
            KeyCode::Char('d') if self.selected_task().is_some() => {
                self.open_delete();
                Some(Effect::None)
            }
            KeyCode::Char('[') => {
                self.scroll_detail(-1);
                Some(Effect::None)
            }
            KeyCode::Char(']') => {
                self.scroll_detail(1);
                Some(Effect::None)
            }
            _ => None,
        }
    }

    fn nav_key(&mut self, key: KeyEvent) -> Effect {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.move_nav(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_nav(1),
            KeyCode::Home | KeyCode::Char('g') => self.jump_nav(false),
            KeyCode::End | KeyCode::Char('G') => self.jump_nav(true),
            KeyCode::PageUp => self.page_nav(false),
            KeyCode::PageDown => self.page_nav(true),
            KeyCode::Char('u') if ctrl => self.page_nav(false),
            KeyCode::Char('d') if ctrl => self.page_nav(true),
            KeyCode::Char('>') => self.enter_detail(),
            KeyCode::Right
            | KeyCode::Char('l')
            | KeyCode::Enter
            | KeyCode::Tab
            | KeyCode::BackTab => {
                self.focus = Focus::Tasks;
                Effect::None
            }
            _ => Effect::None,
        }
    }

    fn task_key(&mut self, key: KeyEvent) -> Effect {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.move_task(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_task(1),
            KeyCode::Home | KeyCode::Char('g') => self.jump_task(false),
            KeyCode::End | KeyCode::Char('G') => self.jump_task(true),
            KeyCode::PageUp => self.page_task(false),
            KeyCode::PageDown => self.page_task(true),
            KeyCode::Char('u') if ctrl => self.page_task(false),
            KeyCode::Char('d') if ctrl => self.page_task(true),
            KeyCode::Enter | KeyCode::Char('>') => self.enter_detail(),
            KeyCode::Left | KeyCode::Char('h') | KeyCode::Tab | KeyCode::BackTab => {
                self.focus = Focus::Nav;
                Effect::None
            }
            _ => Effect::None,
        }
    }

    fn enter_detail(&mut self) -> Effect {
        if self.selected_task().is_none() {
            return Effect::None;
        }
        self.focus = Focus::Detail;
        Effect::None
    }

    fn detail_key(&mut self, key: KeyEvent) -> Effect {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let page = (self.detail_height / 2).max(1) as isize;
        match key.code {
            KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') | KeyCode::Char('<') => {
                self.focus = Focus::Tasks;
                Effect::None
            }
            KeyCode::Tab | KeyCode::BackTab => {
                self.focus = Focus::Nav;
                Effect::None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.scroll_detail(-1);
                Effect::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.scroll_detail(1);
                Effect::None
            }
            KeyCode::PageUp => {
                self.scroll_detail(-page);
                Effect::None
            }
            KeyCode::PageDown => {
                self.scroll_detail(page);
                Effect::None
            }
            KeyCode::Char('u') if ctrl => {
                self.scroll_detail(-page);
                Effect::None
            }
            KeyCode::Char('d') if ctrl => {
                self.scroll_detail(page);
                Effect::None
            }
            KeyCode::Home | KeyCode::Char('g') => {
                self.scroll_detail(-10_000);
                Effect::None
            }
            KeyCode::End | KeyCode::Char('G') => {
                self.scroll_detail(10_000);
                Effect::None
            }
            _ => Effect::None,
        }
    }

    fn filter_key(&mut self, key: KeyEvent) -> Effect {
        match key.code {
            KeyCode::Esc => {
                self.filter = Editor::new("");
                self.focus = Focus::Tasks;
                self.clamp_selection();
                self.task_scroll = 0;
                self.reveal_task();
                Effect::None
            }
            KeyCode::Enter | KeyCode::Tab | KeyCode::BackTab => {
                self.focus = Focus::Tasks;
                Effect::None
            }
            KeyCode::Up => self.move_task(-1),
            KeyCode::Down => self.move_task(1),
            _ => {
                if edit_key(&mut self.filter, &key) {
                    self.clamp_selection();
                    self.task_scroll = 0;
                    self.reveal_task();
                }
                Effect::None
            }
        }
    }

    fn overlay_key(&mut self, key: KeyEvent) -> Effect {
        match &self.overlay {
            Some(Overlay::Help) => self.help_key(key),
            Some(Overlay::Add(_)) => self.add_key(key),
            Some(Overlay::Due(_)) => self.due_key(key),
            Some(Overlay::Delete(_)) => self.delete_key(key),
            None => Effect::None,
        }
    }

    fn help_key(&mut self, key: KeyEvent) -> Effect {
        match key.code {
            KeyCode::Esc | KeyCode::Char('?') => {
                self.overlay = None;
                Effect::None
            }
            KeyCode::Char('q') => Effect::Quit,
            _ => Effect::None,
        }
    }

    fn add_key(&mut self, key: KeyEvent) -> Effect {
        let busy = matches!(&self.overlay, Some(Overlay::Add(form)) if form.busy);
        if busy {
            return Effect::None;
        }
        match key.code {
            KeyCode::Esc => {
                self.overlay = None;
                Effect::None
            }
            KeyCode::Enter => self.submit_add(),
            _ => {
                if let Some(Overlay::Add(form)) = &mut self.overlay {
                    edit_key(&mut form.text, &key);
                    form.error = None;
                }
                Effect::None
            }
        }
    }

    fn due_key(&mut self, key: KeyEvent) -> Effect {
        let busy = matches!(&self.overlay, Some(Overlay::Due(form)) if form.busy);
        if busy {
            return Effect::None;
        }
        match key.code {
            KeyCode::Esc => {
                self.overlay = None;
                Effect::None
            }
            KeyCode::Enter => self.submit_due(),
            _ => {
                if let Some(Overlay::Due(form)) = &mut self.overlay {
                    edit_key(&mut form.due, &key);
                    form.error = None;
                }
                Effect::None
            }
        }
    }

    fn delete_key(&mut self, key: KeyEvent) -> Effect {
        let busy = matches!(&self.overlay, Some(Overlay::Delete(form)) if form.busy);
        if busy {
            return Effect::None;
        }
        match key.code {
            KeyCode::Esc => {
                self.overlay = None;
                Effect::None
            }
            KeyCode::Enter => self.submit_delete(),
            KeyCode::Char('q') => Effect::Quit,
            _ => Effect::None,
        }
    }

    fn open_add(&mut self) {
        self.add_generation += 1;
        let hint = match &self.view {
            View::Project(id) => {
                let name = self
                    .project_named(id)
                    .map(|project| project.name.as_str())
                    .unwrap_or("this project");
                format!("Adds to {name} unless the text names a #project")
            }
            _ => "Adds to Inbox unless the text names a #project".to_string(),
        };
        self.overlay = Some(Overlay::Add(AddForm {
            text: Editor::new(""),
            hint,
            error: None,
            busy: false,
            generation: self.add_generation,
        }));
    }

    fn open_due(&mut self) {
        let Some((task_id, content, due)) = self.selected_task().map(|task| {
            (
                task.id.clone(),
                task.content.clone(),
                task.due
                    .as_ref()
                    .map(|due| due.string.clone())
                    .unwrap_or_default(),
            )
        }) else {
            return;
        };
        self.due_generation += 1;
        self.overlay = Some(Overlay::Due(DueForm {
            task_id,
            content,
            due: Editor::new(due),
            error: None,
            busy: false,
            generation: self.due_generation,
        }));
    }

    fn open_delete(&mut self) {
        let Some((task_id, content)) = self
            .selected_task()
            .map(|task| (task.id.clone(), task.content.clone()))
        else {
            return;
        };
        self.delete_generation += 1;
        self.overlay = Some(Overlay::Delete(DeleteForm {
            task_id,
            content,
            error: None,
            busy: false,
            generation: self.delete_generation,
        }));
    }

    fn submit_add(&mut self) -> Effect {
        let Some(Overlay::Add(form)) = &self.overlay else {
            return Effect::None;
        };
        if form.busy {
            return Effect::None;
        }
        let typed = form.text.value.trim().to_string();
        let generation = form.generation;
        if typed.is_empty() {
            if let Some(Overlay::Add(form)) = &mut self.overlay {
                form.error = Some("Enter a task".to_string());
            }
            return Effect::None;
        }
        let project = match &self.view.clone() {
            View::Project(id) => self
                .projects
                .iter()
                .find(|project| project.id == *id)
                .map(|project| project.name.clone()),
            _ => None,
        };
        let text = quick_add_text(&typed, project.as_deref());
        if let Some(Overlay::Add(form)) = &mut self.overlay {
            form.busy = true;
            form.error = None;
        }
        self.begin_mutation();
        Effect::Jobs(vec![Job::QuickAdd { generation, text }])
    }

    fn submit_due(&mut self) -> Effect {
        let Some(Overlay::Due(form)) = &self.overlay else {
            return Effect::None;
        };
        if form.busy {
            return Effect::None;
        }
        let due = form.due.value.trim().to_string();
        let id = form.task_id.clone();
        let generation = form.generation;
        if due.is_empty() {
            if let Some(Overlay::Due(form)) = &mut self.overlay {
                form.error = Some("Enter a date, like tomorrow or next monday".to_string());
            }
            return Effect::None;
        }
        if let Some(Overlay::Due(form)) = &mut self.overlay {
            form.busy = true;
            form.error = None;
        }
        self.begin_mutation();
        Effect::Jobs(vec![Job::Reschedule {
            generation,
            id,
            due,
        }])
    }

    fn submit_delete(&mut self) -> Effect {
        let Some(Overlay::Delete(form)) = &self.overlay else {
            return Effect::None;
        };
        if form.busy {
            return Effect::None;
        }
        let id = form.task_id.clone();
        let generation = form.generation;
        if let Some(Overlay::Delete(form)) = &mut self.overlay {
            form.busy = true;
            form.error = None;
        }
        self.begin_mutation();
        Effect::Jobs(vec![Job::Delete { generation, id }])
    }

    fn close_selected(&mut self) -> Effect {
        let Some(id) = self.selected_task().map(|task| task.id.clone()) else {
            return Effect::None;
        };
        if !self.closing.insert(id.clone()) {
            return Effect::None;
        }
        self.begin_mutation();
        Effect::Jobs(vec![Job::Close { id }])
    }

    fn move_nav(&mut self, delta: isize) -> Effect {
        let rows = self.navigation();
        let positions = selectable(&rows);
        if positions.is_empty() {
            return Effect::None;
        }
        let current = positions
            .iter()
            .position(|&index| entry_is(&rows[index], &self.view))
            .unwrap_or(0);
        let next = (current as isize + delta).clamp(0, positions.len() as isize - 1) as usize;
        if let NavItem::Entry(view) = &rows[positions[next]].item {
            self.set_view(view.clone());
        }
        self.reveal_nav();
        Effect::None
    }

    fn jump_nav(&mut self, end: bool) -> Effect {
        let rows = self.navigation();
        let positions = selectable(&rows);
        let Some(&index) = (if end {
            positions.last()
        } else {
            positions.first()
        }) else {
            return Effect::None;
        };
        if let NavItem::Entry(view) = &rows[index].item {
            self.set_view(view.clone());
        }
        self.reveal_nav();
        Effect::None
    }

    fn page_nav(&mut self, down: bool) -> Effect {
        let step = (self.nav_height / 2).max(1) as isize;
        self.move_nav(if down { step } else { -step })
    }

    fn move_task(&mut self, delta: isize) -> Effect {
        let indices = self.ordered_tasks();
        if indices.is_empty() {
            return Effect::None;
        }
        let current = self
            .selected_id
            .as_ref()
            .and_then(|id| {
                indices
                    .iter()
                    .position(|&index| self.tasks[index].id == *id)
            })
            .unwrap_or(0);
        let next = (current as isize + delta).clamp(0, indices.len() as isize - 1) as usize;
        let id = self.tasks[indices[next]].id.clone();
        self.select(Some(id));
        self.reveal_task();
        Effect::None
    }

    fn jump_task(&mut self, end: bool) -> Effect {
        let indices = self.ordered_tasks();
        let Some(&index) = (if end { indices.last() } else { indices.first() }) else {
            self.select(None);
            return Effect::None;
        };
        if !end {
            self.task_scroll = 0;
        }
        let id = self.tasks[index].id.clone();
        self.select(Some(id));
        self.reveal_task();
        Effect::None
    }

    fn page_task(&mut self, down: bool) -> Effect {
        let step = (self.task_height / 2).max(1) as isize;
        self.move_task(if down { step } else { -step })
    }

    fn set_view(&mut self, view: View) {
        if view == self.view {
            return;
        }
        if let Some(id) = &self.selected_id {
            self.selections.insert(self.view.clone(), id.clone());
        }
        self.view = view;
        self.selected_id = self.selections.get(&self.view).cloned();
        self.task_scroll = 0;
        self.detail_scroll = 0;
        self.clamp_selection();
        self.reveal_task();
    }

    /// Move the highlight. The detail pane scrolls back to the top when the task changes.
    fn select(&mut self, id: Option<String>) {
        if self.selected_id != id {
            self.detail_scroll = 0;
        }
        self.selected_id = id;
    }

    pub(crate) fn clamp_selection(&mut self) {
        let indices = self.ordered_tasks();
        if let Some(id) = &self.selected_id
            && indices.iter().any(|&index| self.tasks[index].id == *id)
        {
            return;
        }
        let id = indices.first().map(|&index| self.tasks[index].id.clone());
        self.select(id);
    }

    fn reveal_task(&mut self) {
        let lines = self.line_layout();
        let height = self.task_height;
        if height == 0 {
            return;
        }
        let id = self.selected_id.as_deref();
        let start = lines.iter().position(|line| match line {
            LineKind::TaskTop(index) => id == Some(self.tasks[*index].id.as_str()),
            _ => false,
        });
        let Some(start) = start else {
            return;
        };
        let mut end = start + 1;
        if matches!(lines.get(start + 1), Some(LineKind::TaskBody(_))) {
            end = start + 2;
        }
        if start < self.task_scroll {
            self.task_scroll = start;
        } else if end > self.task_scroll + height {
            self.task_scroll = end - height;
        }
    }

    fn reveal_nav(&mut self) {
        let rows = self.navigation();
        let Some(pos) = rows.iter().position(|row| entry_is(row, &self.view)) else {
            return;
        };
        let height = self.nav_height;
        if height == 0 {
            return;
        }
        if pos < self.nav_offset {
            self.nav_offset = pos;
        } else if pos >= self.nav_offset + height {
            self.nav_offset = pos + 1 - height;
        }
    }

    fn scroll_detail(&mut self, delta: isize) {
        let max = self.detail_rows.saturating_sub(self.detail_height);
        self.detail_scroll = (self.detail_scroll as isize + delta).clamp(0, max as isize) as usize;
    }

    fn click(&mut self, x: u16, y: u16) -> Effect {
        if self.overlay.is_some() {
            return Effect::None;
        }
        let hits = self.screen.hits.clone();
        if clicked(hits.add, x, y) {
            self.open_add();
            return Effect::None;
        }
        if clicked(hits.refresh, x, y) {
            return self.refresh();
        }
        if clicked(hits.done, x, y) {
            return self.close_selected();
        }
        if clicked(hits.due, x, y) {
            self.open_due();
            return Effect::None;
        }
        if clicked(hits.delete, x, y) {
            self.open_delete();
            return Effect::None;
        }
        if clicked(hits.filter, x, y) {
            self.focus = Focus::Filter;
            return Effect::None;
        }
        if let Some(index) = hit(&hits.nav_rows, x, y) {
            let rows = self.navigation();
            if let Some(row) = rows.get(index)
                && let NavItem::Entry(view) = &row.item
            {
                self.focus = Focus::Nav;
                self.set_view(view.clone());
                self.reveal_nav();
            }
            return Effect::None;
        }
        if let Some(area) = hits.detail_area
            && contains(area, x, y)
        {
            if self.selected_task().is_some() {
                self.focus = Focus::Detail;
            }
            return Effect::None;
        }
        if let Some(TaskHit::Task(index)) = hit(&hits.task_rows, x, y) {
            self.focus = Focus::Tasks;
            if let Some(task) = self.tasks.get(index) {
                let id = task.id.clone();
                self.select(Some(id));
                self.reveal_task();
            }
        }
        Effect::None
    }

    fn scroll_at(&mut self, x: u16, y: u16, notches: isize) -> Effect {
        if self.overlay.is_some() {
            return Effect::None;
        }
        let hits = self.screen.hits.clone();
        if let Some(area) = hits.detail_area
            && contains(area, x, y)
        {
            self.scroll_detail(notches.signum() * 3);
            return Effect::None;
        }
        if let Some(area) = hits.nav_area
            && contains(area, x, y)
        {
            let step = notches.signum();
            let max = self.nav_len.saturating_sub(self.nav_height);
            self.nav_offset = (self.nav_offset as isize + step).clamp(0, max as isize) as usize;
            return Effect::None;
        }
        if let Some(area) = hits.task_area
            && contains(area, x, y)
        {
            let step = notches.signum() * 3;
            let max = self.task_len.saturating_sub(self.task_height);
            self.task_scroll = (self.task_scroll as isize + step).clamp(0, max as isize) as usize;
        }
        Effect::None
    }

    fn focused_editor(&mut self) -> Option<&mut Editor> {
        if let Some(overlay) = &mut self.overlay {
            return match overlay {
                Overlay::Add(form) if !form.busy => Some(&mut form.text),
                Overlay::Due(form) if !form.busy => Some(&mut form.due),
                _ => None,
            };
        }
        if self.focus == Focus::Filter {
            Some(&mut self.filter)
        } else {
            None
        }
    }
}

fn clicked(rect: Option<ratatui::layout::Rect>, x: u16, y: u16) -> bool {
    rect.is_some_and(|rect| contains(rect, x, y))
}

fn selectable(rows: &[NavRow]) -> Vec<usize> {
    rows.iter()
        .enumerate()
        .filter_map(|(index, row)| matches!(row.item, NavItem::Entry(_)).then_some(index))
        .collect()
}

fn entry_is(row: &NavRow, view: &View) -> bool {
    matches!(&row.item, NavItem::Entry(item) if item == view)
}

fn edit_key(editor: &mut Editor, key: &KeyEvent) -> bool {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    match key.code {
        KeyCode::Char('u') if ctrl => editor.kill_to_start(),
        KeyCode::Char('w') if ctrl => editor.delete_word_back(),
        KeyCode::Char('d') if ctrl => editor.delete_forward(),
        KeyCode::Backspace if alt || ctrl => editor.delete_word_back(),
        KeyCode::Backspace => editor.backspace(),
        KeyCode::Delete => editor.delete_forward(),
        KeyCode::Left => editor.move_left(),
        KeyCode::Right => editor.move_right(),
        KeyCode::Home => editor.move_home(),
        KeyCode::End => editor.move_end(),
        KeyCode::Char(ch) if !ctrl && !alt && !ch.is_control() => editor.insert_char(ch),
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::todoist::Due;
    use chrono::NaiveDate;

    fn key(ch: char) -> KeyEvent {
        event(KeyCode::Char(ch))
    }

    fn event(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn task(id: &str, project: &str, content: &str, date: &str) -> Task {
        Task {
            id: id.to_string(),
            content: content.to_string(),
            description: String::new(),
            project_id: project.to_string(),
            section_id: None,
            parent_id: None,
            priority: 1,
            labels: Vec::new(),
            due: Some(Due {
                date: date.to_string(),
                string: date.to_string(),
                is_recurring: false,
            }),
            child_order: 0,
            day_order: -1,
            note_count: 0,
            checked: false,
            is_deleted: false,
        }
    }

    #[allow(clippy::field_reassign_with_default)]
    fn fixture() -> App {
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
            task("old", "in", "Call Mom", "2026-10-09"),
            task("milk", "personal", "Buy milk", "2026-10-16"),
            task("later", "personal", "Change oil", "2026-10-20"),
        ];
        app.task_height = 20;
        app.nav_height = 20;
        app.clamp_selection();
        app
    }

    #[test]
    fn today_starts_on_the_first_due_task_and_j_moves() {
        let mut app = fixture();
        assert_eq!(app.view, View::Today);
        assert_eq!(app.selected_task().unwrap().content, "Call Mom");
        app.on_key(key('j'));
        assert_eq!(app.selected_task().unwrap().content, "Buy milk");
    }

    #[test]
    fn x_queues_a_close_for_the_selected_task() {
        let mut app = fixture();
        let effect = app.on_key(key('x'));
        match effect {
            Effect::Jobs(jobs) => match &jobs[0] {
                Job::Close { id } => assert_eq!(id, "old"),
                other => panic!("unexpected job {other:?}"),
            },
            Effect::None => panic!("no job"),
            Effect::Quit => panic!("quit"),
        }
    }

    #[test]
    fn the_filter_hides_tasks_that_do_not_match() {
        let mut app = fixture();
        app.focus = Focus::Filter;
        app.on_key(key('m'));
        app.on_key(key('i'));
        app.on_key(key('l'));
        app.on_key(key('k'));
        assert_eq!(app.shown_count(), 1);
        assert_eq!(app.selected_task().unwrap().content, "Buy milk");
    }

    #[test]
    fn moving_to_a_project_shows_only_its_tasks() {
        let mut app = fixture();
        app.focus = Focus::Nav;
        app.on_key(key('j'));
        app.on_key(key('j'));
        assert!(matches!(app.view, View::Project(ref id) if id == "personal"));
        assert_eq!(app.view_count(), 2);
        assert_eq!(app.selected_task().unwrap().content, "Buy milk");
    }

    #[test]
    fn brackets_scroll_the_detail_and_moving_resets_it() {
        let mut app = fixture();
        app.detail_rows = 20;
        app.detail_height = 5;
        app.on_key(key(']'));
        app.on_key(key(']'));
        assert_eq!(app.detail_scroll, 2);
        app.on_key(key('['));
        assert_eq!(app.detail_scroll, 1);
        app.on_key(key('j'));
        assert_eq!(app.selected_task().unwrap().content, "Buy milk");
        assert_eq!(app.detail_scroll, 0);
    }

    #[test]
    fn enter_and_greater_than_open_the_detail_and_j_scrolls_it() {
        let mut app = fixture();
        app.detail_rows = 40;
        app.detail_height = 8;
        let task = app.selected_task().unwrap().content.clone();
        app.on_key(event(KeyCode::Enter));
        assert_eq!(app.focus, Focus::Detail);
        app.on_key(key('j'));
        app.on_key(key('j'));
        assert_eq!(app.detail_scroll, 2);
        assert_eq!(app.selected_task().unwrap().content, task);
        app.on_key(event(KeyCode::Esc));
        assert_eq!(app.focus, Focus::Tasks);

        app.on_key(key('>'));
        assert_eq!(app.focus, Focus::Detail);
    }

    #[test]
    fn enter_from_the_nav_still_opens_the_list_and_filter_types_greater_than() {
        let mut app = fixture();
        app.focus = Focus::Nav;
        app.on_key(event(KeyCode::Enter));
        assert_eq!(app.focus, Focus::Tasks);

        app.focus = Focus::Nav;
        app.on_key(key('>'));
        assert_eq!(app.focus, Focus::Detail);

        let mut app = fixture();
        app.focus = Focus::Filter;
        app.on_key(key('>'));
        assert_eq!(app.focus, Focus::Filter);
        assert_eq!(app.filter.value, ">");
    }

    #[test]
    fn n_opens_add_and_end_lands_on_the_last_task() {
        let mut app = fixture();
        app.on_key(key('G'));
        assert_eq!(app.selected_task().unwrap().content, "Buy milk");
        app.on_key(key('n'));
        assert!(matches!(app.overlay, Some(Overlay::Add(_))));
    }
}
