use ratatui::layout::Rect;

#[derive(Clone, Debug, Default)]
pub struct Screen {
    pub hits: Hits,
    pub nav_height: usize,
    pub nav_offset: usize,
    pub nav_len: usize,
    pub task_height: usize,
    pub task_scroll: usize,
    pub task_len: usize,
    pub detail_rows: usize,
    pub detail_height: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Hits {
    pub add: Option<Rect>,
    pub refresh: Option<Rect>,
    pub done: Option<Rect>,
    pub due: Option<Rect>,
    pub delete: Option<Rect>,
    pub filter: Option<Rect>,
    pub nav_area: Option<Rect>,
    pub task_area: Option<Rect>,
    pub detail_area: Option<Rect>,
    pub nav_rows: Vec<(Rect, usize)>,
    pub task_rows: Vec<(Rect, TaskHit)>,
    pub modal: Option<Rect>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskHit {
    Task(usize),
}

pub fn contains(rect: Rect, x: u16, y: u16) -> bool {
    x >= rect.x && y >= rect.y && x < rect.right() && y < rect.bottom()
}

pub fn hit<T: Copy>(rects: &[(Rect, T)], x: u16, y: u16) -> Option<T> {
    rects
        .iter()
        .rev()
        .find(|(rect, _)| contains(*rect, x, y))
        .map(|(_, value)| *value)
}
