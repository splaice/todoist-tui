use std::io::Read;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

const BASE: &str = "https://api.todoist.com/api/v1";

#[derive(Clone, Debug)]
pub struct Client {
    token: String,
    base: String,
}

#[derive(Clone, Debug)]
pub struct SyncData {
    pub user: User,
    pub projects: Vec<Project>,
    pub tasks: Vec<Task>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct User {
    #[serde(default)]
    pub full_name: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub inbox_project_id: String,
}

impl User {
    pub fn label(&self) -> String {
        let name = self.full_name.trim();
        if !name.is_empty() {
            return name.to_string();
        }
        if !self.email.trim().is_empty() {
            return self.email.trim().to_string();
        }
        "Todoist".to_string()
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Project {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub child_order: i64,
    #[serde(default = "grey")]
    pub color: String,
    #[serde(default)]
    pub is_inbox_project: bool,
    #[serde(default)]
    pub is_archived: bool,
    #[serde(default)]
    pub is_deleted: bool,
}

fn grey() -> String {
    "grey".to_string()
}

impl Project {
    pub fn parent(&self) -> Option<&str> {
        self.parent_id.as_deref().filter(|id| !id.is_empty())
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Task {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub section_id: Option<String>,
    #[serde(default)]
    pub parent_id: Option<String>,
    /// Todoist stores p1 (urgent) as 4 and p4 (normal) as 1.
    #[serde(default = "normal_priority")]
    pub priority: i32,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub due: Option<Due>,
    #[serde(default)]
    pub child_order: i64,
    #[serde(default = "unset_day_order")]
    pub day_order: i64,
    #[serde(default)]
    pub note_count: u32,
    #[serde(default)]
    pub checked: bool,
    #[serde(default)]
    pub is_deleted: bool,
}

fn normal_priority() -> i32 {
    1
}

fn unset_day_order() -> i64 {
    -1
}

impl Task {
    pub fn parent(&self) -> Option<&str> {
        self.parent_id.as_deref().filter(|id| !id.is_empty())
    }

    pub fn active(&self) -> bool {
        !self.checked && !self.is_deleted && !self.id.is_empty()
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Due {
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub string: String,
    #[serde(default)]
    pub is_recurring: bool,
}

impl Client {
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
            base: BASE.to_string(),
        }
    }

    pub fn from_env() -> Result<Self, String> {
        match std::env::var("TODOIST_API_TOKEN") {
            Ok(token) if !token.trim().is_empty() => Ok(Self::new(token.trim())),
            _ => Err(
                "TODOIST_API_TOKEN is not set. Create one in Todoist → Settings → Integrations → Developer."
                    .to_string(),
            ),
        }
    }

    pub fn sync(&self) -> Result<SyncData, String> {
        let user = self.user()?;
        let projects = self.projects()?;
        let mut tasks = self.tasks()?;
        tasks.retain(Task::active);
        Ok(SyncData {
            user,
            projects,
            tasks,
        })
    }

    pub fn user(&self) -> Result<User, String> {
        let body = self.get("/user", &[])?;
        serde_json::from_slice(&body).map_err(|err| format!("parsing user: {err}"))
    }

    pub fn projects(&self) -> Result<Vec<Project>, String> {
        self.pages("/projects", &[])
    }

    pub fn tasks(&self) -> Result<Vec<Task>, String> {
        self.pages("/tasks", &[])
    }

    pub fn quick_add(&self, text: &str) -> Result<Option<Task>, String> {
        let body = serde_json::json!({ "text": text, "meta": false });
        let bytes = self.post_json("/tasks/quick", &body)?;
        parse_quick_add(&bytes)
    }

    pub fn close(&self, id: &str) -> Result<(), String> {
        self.post_empty(&format!("/tasks/{}/close", encode_path(id)))
    }

    pub fn delete(&self, id: &str) -> Result<(), String> {
        self.delete_path(&format!("/tasks/{}", encode_path(id)))
    }

    pub fn reschedule(&self, id: &str, due: &str) -> Result<Task, String> {
        #[derive(Serialize)]
        struct Body<'a> {
            due_string: &'a str,
        }
        let bytes = self.post_json(
            &format!("/tasks/{}", encode_path(id)),
            &Body { due_string: due },
        )?;
        serde_json::from_slice(&bytes).map_err(|err| format!("parsing task: {err}"))
    }

    fn pages<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<Vec<T>, String> {
        let mut all = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let mut params = query.to_vec();
            if let Some(cursor) = &cursor {
                params.push(("cursor", cursor.clone()));
            }
            let body = self.get(path, &params)?;
            let (items, next) = parse_page(&body)?;
            all.extend(items);
            match next {
                Some(next) if !next.is_empty() => cursor = Some(next),
                _ => break,
            }
            if all.len() > 20_000 {
                break;
            }
        }
        Ok(all)
    }

    fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Vec<u8>, String> {
        let mut request = self.request("GET", path);
        for (key, value) in query {
            request = request.query(key, value);
        }
        self.send(request.call())
    }

    fn post_json(&self, path: &str, body: &impl Serialize) -> Result<Vec<u8>, String> {
        let payload = serde_json::to_string(body).map_err(|err| err.to_string())?;
        let request = self
            .request("POST", path)
            .set("Content-Type", "application/json");
        self.send(request.send_string(&payload))
    }

    fn post_empty(&self, path: &str) -> Result<(), String> {
        self.send(self.request("POST", path).call()).map(|_| ())
    }

    fn delete_path(&self, path: &str) -> Result<(), String> {
        self.send(self.request("DELETE", path).call()).map(|_| ())
    }

    fn request(&self, method: &str, path: &str) -> ureq::Request {
        ureq::request(method, &format!("{}{path}", self.base))
            .set("Authorization", &format!("Bearer {}", self.token))
            .set("User-Agent", "todoist-tui")
    }

    fn send(&self, result: Result<ureq::Response, ureq::Error>) -> Result<Vec<u8>, String> {
        match result {
            Ok(response) => read_body(response),
            Err(err) => Err(http_error(&self.token, err)),
        }
    }
}

fn read_body(response: ureq::Response) -> Result<Vec<u8>, String> {
    let status = response.status();
    let mut body = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut body)
        .map_err(|err| err.to_string())?;
    if !(200..300).contains(&status) {
        return Err(format!("API error {status}"));
    }
    Ok(body)
}

fn http_error(token: &str, err: ureq::Error) -> String {
    match err {
        ureq::Error::Status(code, response) => {
            let mut body = String::new();
            let _ = response.into_reader().take(2000).read_to_string(&mut body);
            format!("API error {code}: {}", sanitize(token, &body))
        }
        ureq::Error::Transport(err) => format!("Network error: {err}"),
    }
}

fn sanitize(token: &str, body: &str) -> String {
    let body = if token.is_empty() {
        body.to_string()
    } else {
        body.replace(token, "••••")
    };
    let flat: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out: String = flat.chars().take(180).collect();
    if flat.chars().count() > 180 {
        out.push('…');
    }
    if out.is_empty() {
        "empty response".to_string()
    } else {
        out
    }
}

pub fn parse_page<T: DeserializeOwned>(body: &[u8]) -> Result<(Vec<T>, Option<String>), String> {
    let value: serde_json::Value =
        serde_json::from_slice(body).map_err(|err| format!("invalid JSON: {err}"))?;
    if value.is_array() {
        let items = serde_json::from_value(value).map_err(|err| format!("parsing list: {err}"))?;
        return Ok((items, None));
    }
    let Some(results) = value.get("results") else {
        return Err(format!("unexpected response: {}", compact_json(&value)));
    };
    let items =
        serde_json::from_value(results.clone()).map_err(|err| format!("parsing page: {err}"))?;
    let next_cursor = match value.get("next_cursor") {
        Some(serde_json::Value::String(cursor)) if !cursor.is_empty() => Some(cursor.clone()),
        _ => None,
    };
    Ok((items, next_cursor))
}

fn parse_quick_add(body: &[u8]) -> Result<Option<Task>, String> {
    if body.iter().all(u8::is_ascii_whitespace) {
        return Ok(None);
    }
    let value: serde_json::Value =
        serde_json::from_slice(body).map_err(|err| format!("parsing task: {err}"))?;
    if let Some(task) = task_from_value(&value) {
        return Ok(Some(task));
    }
    for key in ["task", "item"] {
        if let Some(inner) = value.get(key)
            && let Some(task) = task_from_value(inner)
        {
            return Ok(Some(task));
        }
    }
    Ok(None)
}

fn task_from_value(value: &serde_json::Value) -> Option<Task> {
    let task: Task = serde_json::from_value(value.clone()).ok()?;
    if task.id.is_empty() || task.content.is_empty() {
        None
    } else {
        Some(task)
    }
}

fn compact_json(value: &serde_json::Value) -> String {
    let text = value.to_string();
    let mut out: String = text.chars().take(160).collect();
    if text.chars().count() > 160 {
        out.push('…');
    }
    out
}

fn encode_path(id: &str) -> String {
    let mut out = String::new();
    for byte in id.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_task_page_and_stops_on_a_null_cursor() {
        let json = r#"{
            "results": [{
                "id": "abc",
                "content": "Buy milk",
                "project_id": "inbox",
                "priority": 4,
                "due": {"date": "2026-10-16", "string": "today", "is_recurring": false},
                "labels": ["errand"],
                "description": "oat",
                "note_count": 2,
                "child_order": 3,
                "day_order": -1
            }],
            "next_cursor": null
        }"#;
        let (tasks, cursor) = parse_page::<Task>(json.as_bytes()).unwrap();
        assert!(cursor.is_none());
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].content, "Buy milk");
        assert_eq!(tasks[0].priority, 4);
        assert_eq!(tasks[0].note_count, 2);
        assert_eq!(tasks[0].due.as_ref().unwrap().string, "today");
    }

    #[test]
    fn parses_a_bare_array_and_rejects_an_error_object() {
        let array = r#"[{"id":"a","content":"One","project_id":"p"}]"#;
        let (tasks, cursor) = parse_page::<Task>(array.as_bytes()).unwrap();
        assert!(cursor.is_none());
        assert_eq!(tasks[0].content, "One");
        assert_eq!(tasks[0].priority, 1);

        let error = parse_page::<Task>(br#"{"error":"nope"}"#);
        assert!(error.is_err());
    }

    #[test]
    fn quick_add_accepts_a_task_or_an_empty_object() {
        let task =
            parse_quick_add(br#"{"id":"abc","content":"Buy milk","project_id":"p"}"#).unwrap();
        assert_eq!(task.unwrap().content, "Buy milk");
        let wrapped = parse_quick_add(
            br#"{"task":{"id":"abc","content":"Call","project_id":"p"},"meta":{}}"#,
        )
        .unwrap();
        assert_eq!(wrapped.unwrap().content, "Call");
        assert!(parse_quick_add(b"{}").unwrap().is_none());
    }

    #[test]
    fn sanitize_strips_the_token_and_flattens_the_body() {
        let text = sanitize("secret-token", "nope secret-token\nstill");
        assert_eq!(text, "nope •••• still");
    }
}
