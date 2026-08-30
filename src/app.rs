use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::Value;
use tokio::sync::mpsc;

use crate::{
    config::Config,
    diff::{display_content, json_diff, text_diff},
    schema::{
        FieldKind, FormMode, build_payload, form_for, is_destructive_method, redacted_payload,
    },
    swamp::{
        DataArtifact, DataContent, DataVersion, MethodSpec, ModelDetails, ModelSummary, RunEvent,
        SwampClient, TypeDescription,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Overview,
    Methods,
    Data,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Models,
    Content,
}

#[derive(Debug, Clone)]
pub enum LargeAction {
    Latest,
    Version(u64),
    Diff(u64, u64),
}

#[derive(Debug, Clone)]
pub enum InputMode {
    Normal,
    Search,
    MethodForm,
    Review,
    DestructiveConfirm(String),
    LargeConfirm(LargeAction),
    Help,
}

#[derive(Debug, Clone)]
pub struct MethodForm {
    pub method: MethodSpec,
    pub mode: FormMode,
    pub selected_field: usize,
}

pub struct App {
    pub config: Config,
    pub client: Arc<dyn SwampClient>,
    pub swamp_version: String,
    pub models: Vec<ModelSummary>,
    pub filtered_models: Vec<usize>,
    pub model_index: usize,
    pub detail: Option<ModelDetails>,
    pub type_description: Option<TypeDescription>,
    pub method_index: usize,
    pub artifacts: Vec<DataArtifact>,
    pub artifact_index: usize,
    pub content: Option<DataContent>,
    pub versions: Vec<DataVersion>,
    pub version_cursor: usize,
    pub compare_a: Option<usize>,
    pub compare_b: Option<usize>,
    pub diff: Option<String>,
    pub tab: Tab,
    pub focus: Focus,
    pub mode: InputMode,
    pub search: String,
    pub form: Option<MethodForm>,
    pub pending_payload: Option<Value>,
    pub status: String,
    pub error: Option<String>,
    pub run_logs: Vec<String>,
    pub run_receiver: Option<mpsc::UnboundedReceiver<RunEvent>>,
    pub running_model: Option<String>,
    pub should_quit: bool,
    refresh_after_run: bool,
}

impl App {
    pub fn new(config: Config, client: Arc<dyn SwampClient>) -> Self {
        Self {
            config,
            client,
            swamp_version: String::new(),
            models: Vec::new(),
            filtered_models: Vec::new(),
            model_index: 0,
            detail: None,
            type_description: None,
            method_index: 0,
            artifacts: Vec::new(),
            artifact_index: 0,
            content: None,
            versions: Vec::new(),
            version_cursor: 0,
            compare_a: None,
            compare_b: None,
            diff: None,
            tab: Tab::Overview,
            focus: Focus::Models,
            mode: InputMode::Normal,
            search: String::new(),
            form: None,
            pending_payload: None,
            status: "Starting…".to_owned(),
            error: None,
            run_logs: Vec::new(),
            run_receiver: None,
            running_model: None,
            should_quit: false,
            refresh_after_run: false,
        }
    }

    pub async fn load(&mut self) {
        match self.client.version().await {
            Ok(version) => self.swamp_version = version,
            Err(error) => {
                self.fail(error.to_string());
                return;
            }
        }
        self.refresh_models().await;
    }

    pub fn visible_models(&self) -> impl Iterator<Item = &ModelSummary> {
        self.filtered_models
            .iter()
            .filter_map(|index| self.models.get(*index))
    }

    pub fn selected_model(&self) -> Option<&ModelSummary> {
        self.filtered_models
            .get(self.model_index)
            .and_then(|index| self.models.get(*index))
    }

    pub fn selected_method(&self) -> Option<&MethodSpec> {
        self.type_description
            .as_ref()
            .and_then(|description| description.methods.get(self.method_index))
            .or_else(|| {
                self.detail
                    .as_ref()
                    .and_then(|detail| detail.methods.get(self.method_index))
            })
    }

    pub fn selected_artifact(&self) -> Option<&DataArtifact> {
        self.artifacts.get(self.artifact_index)
    }

    pub async fn handle_key(&mut self, key: KeyEvent) {
        self.error = None;
        match self.mode.clone() {
            InputMode::Normal => self.handle_normal(key).await,
            InputMode::Search => self.handle_search(key).await,
            InputMode::MethodForm => self.handle_form(key).await,
            InputMode::Review => self.handle_review(key).await,
            InputMode::DestructiveConfirm(text) => {
                self.handle_destructive_confirm(key, text).await;
            }
            InputMode::LargeConfirm(action) => self.handle_large_confirm(key, action).await,
            InputMode::Help => {
                if matches!(
                    key.code,
                    KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')
                ) {
                    self.mode = InputMode::Normal;
                }
            }
        }
    }

    pub async fn tick(&mut self) {
        let mut completed = None;
        if let Some(receiver) = self.run_receiver.as_mut() {
            while let Ok(event) = receiver.try_recv() {
                match event {
                    RunEvent::Log(line) => {
                        self.run_logs.push(line);
                        if self.run_logs.len() > 2_000 {
                            self.run_logs.remove(0);
                        }
                    }
                    RunEvent::Finished {
                        success,
                        lock_contended: _,
                        message,
                    } => completed = Some((success, message)),
                }
            }
        }
        if let Some((success, message)) = completed {
            self.status = message;
            self.run_receiver = None;
            self.running_model = None;
            self.refresh_after_run = success;
        }
        if self.refresh_after_run {
            self.refresh_after_run = false;
            self.refresh_data().await;
        }
    }

    pub fn content_text(&self) -> Option<String> {
        self.content
            .as_ref()
            .and_then(|content| display_content(&content.content_type, &content.content))
    }

    async fn handle_normal(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.cancel_run().await;
            return;
        }
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('?') => self.mode = InputMode::Help,
            KeyCode::Char('/') => {
                self.search.clear();
                self.mode = InputMode::Search;
            }
            KeyCode::Char('1') if self.tab != Tab::Data || self.focus == Focus::Models => {
                self.tab = Tab::Overview
            }
            KeyCode::Char('2') if self.tab != Tab::Data || self.focus == Focus::Models => {
                self.tab = Tab::Methods
            }
            KeyCode::Char('3') if self.tab != Tab::Data || self.focus == Focus::Models => {
                self.tab = Tab::Data
            }
            KeyCode::Tab | KeyCode::BackTab => {
                self.focus = match self.focus {
                    Focus::Models => Focus::Content,
                    Focus::Content => Focus::Models,
                }
            }
            KeyCode::Char('r') => {
                if self.focus == Focus::Models {
                    self.refresh_models().await;
                } else {
                    self.refresh_current().await;
                }
            }
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1).await,
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1).await,
            KeyCode::Enter => self.open_selected().await,
            KeyCode::Char('c') if self.run_receiver.is_some() => self.cancel_run().await,
            KeyCode::Char('[') if self.tab == Tab::Data => self.move_version(-1),
            KeyCode::Char(']') if self.tab == Tab::Data => self.move_version(1),
            KeyCode::Char('a') if self.tab == Tab::Data => {
                self.compare_a = self
                    .versions
                    .get(self.version_cursor)
                    .map(|_| self.version_cursor)
            }
            KeyCode::Char('b') if self.tab == Tab::Data => {
                self.compare_b = self
                    .versions
                    .get(self.version_cursor)
                    .map(|_| self.version_cursor);
                self.compare_versions().await;
            }
            _ => {}
        }
    }

    async fn handle_search(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.search.clear();
                self.apply_filter();
                self.mode = InputMode::Normal;
            }
            KeyCode::Enter => {
                self.mode = InputMode::Normal;
                self.model_index = 0;
                self.load_selected_model().await;
            }
            KeyCode::Backspace => {
                self.search.pop();
                self.apply_filter();
            }
            KeyCode::Char(character) => {
                self.search.push(character);
                self.apply_filter();
            }
            _ => {}
        }
    }

    async fn handle_form(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Esc {
            self.form = None;
            self.mode = InputMode::Normal;
            return;
        }
        let Some(form) = self.form.as_mut() else {
            self.mode = InputMode::Normal;
            return;
        };
        match &mut form.mode {
            FormMode::RawJson(text) => match key.code {
                KeyCode::Enter => self.review_form().await,
                KeyCode::Backspace => {
                    text.pop();
                }
                KeyCode::Char(character) => text.push(character),
                _ => {}
            },
            FormMode::Fields(fields) => {
                if fields.is_empty() {
                    if key.code == KeyCode::Enter {
                        self.review_form().await;
                    }
                    return;
                }
                match key.code {
                    KeyCode::Up => {
                        form.selected_field = form.selected_field.saturating_sub(1);
                    }
                    KeyCode::Down | KeyCode::Tab => {
                        form.selected_field = (form.selected_field + 1).min(fields.len() - 1);
                    }
                    KeyCode::Enter => self.review_form().await,
                    KeyCode::Left | KeyCode::Right | KeyCode::Char(' ')
                        if matches!(
                            fields[form.selected_field].kind,
                            FieldKind::Boolean | FieldKind::Enum(_)
                        ) =>
                    {
                        cycle_field(&mut fields[form.selected_field]);
                    }
                    KeyCode::Backspace => {
                        fields[form.selected_field].value.pop();
                    }
                    KeyCode::Char(character) => {
                        fields[form.selected_field].value.push(character);
                    }
                    _ => {}
                }
            }
        }
    }

    async fn handle_review(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('n') => self.mode = InputMode::MethodForm,
            KeyCode::Char('y') | KeyCode::Enter => {
                let destructive = self
                    .form
                    .as_ref()
                    .is_some_and(|form| is_destructive_method(&form.method.name));
                if destructive {
                    self.mode = InputMode::DestructiveConfirm(String::new());
                } else {
                    self.execute_pending().await;
                }
            }
            _ => {}
        }
    }

    async fn handle_destructive_confirm(&mut self, key: KeyEvent, mut text: String) {
        match key.code {
            KeyCode::Esc => self.mode = InputMode::Review,
            KeyCode::Backspace => {
                text.pop();
                self.mode = InputMode::DestructiveConfirm(text);
            }
            KeyCode::Enter => {
                if self
                    .selected_model()
                    .is_some_and(|model| model.name == text)
                {
                    self.execute_pending().await;
                } else {
                    self.error = Some("The confirmation must exactly match the model name".into());
                    self.mode = InputMode::DestructiveConfirm(text);
                }
            }
            KeyCode::Char(character) => {
                text.push(character);
                self.mode = InputMode::DestructiveConfirm(text);
            }
            _ => self.mode = InputMode::DestructiveConfirm(text),
        }
    }

    async fn handle_large_confirm(&mut self, key: KeyEvent, action: LargeAction) {
        match key.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                self.mode = InputMode::Normal;
                self.perform_large_action(action).await;
            }
            KeyCode::Esc | KeyCode::Char('n') => self.mode = InputMode::Normal,
            _ => {}
        }
    }

    async fn refresh_models(&mut self) {
        self.status = "Loading models…".to_owned();
        match self.client.models().await {
            Ok(models) => {
                self.models = models;
                self.apply_filter();
                self.model_index = self
                    .model_index
                    .min(self.filtered_models.len().saturating_sub(1));
                self.status = format!("{} model(s)", self.models.len());
                self.load_selected_model().await;
            }
            Err(error) => self.fail(error.to_string()),
        }
    }

    async fn load_selected_model(&mut self) {
        let Some(model) = self.selected_model().cloned() else {
            self.detail = None;
            self.type_description = None;
            self.artifacts.clear();
            self.status = "No models found".to_owned();
            return;
        };
        self.status = format!("Loading {}…", model.name);
        let (detail, description, data) = tokio::join!(
            self.client.model(&model.name),
            self.client.describe_type(&model.model_type),
            self.client.data(&model.name),
        );
        match detail {
            Ok(value) => self.detail = Some(value),
            Err(error) => self.fail(error.to_string()),
        }
        match description {
            Ok(value) => self.type_description = Some(value),
            Err(error) => self.fail(error.to_string()),
        }
        match data {
            Ok(value) => self.artifacts = value,
            Err(error) => self.fail(error.to_string()),
        }
        self.method_index = 0;
        self.artifact_index = 0;
        self.clear_data_view();
        if self.error.is_none() {
            self.status = format!("Loaded {}", model.name);
        }
    }

    async fn refresh_current(&mut self) {
        match self.tab {
            Tab::Overview | Tab::Methods => self.load_selected_model().await,
            Tab::Data => self.refresh_data().await,
        }
    }

    async fn refresh_data(&mut self) {
        let Some(model) = self.selected_model().map(|model| model.name.clone()) else {
            return;
        };
        match self.client.data(&model).await {
            Ok(artifacts) => {
                self.artifacts = artifacts;
                self.artifact_index = self
                    .artifact_index
                    .min(self.artifacts.len().saturating_sub(1));
                self.status = "Data refreshed".to_owned();
            }
            Err(error) => self.fail(error.to_string()),
        }
    }

    async fn move_selection(&mut self, direction: isize) {
        if self.focus == Focus::Models {
            let previous = self.model_index;
            self.model_index = move_index(self.model_index, self.filtered_models.len(), direction);
            if previous != self.model_index {
                self.load_selected_model().await;
            }
            return;
        }
        match self.tab {
            Tab::Overview => {}
            Tab::Methods => {
                let length = self
                    .type_description
                    .as_ref()
                    .map_or(0, |description| description.methods.len());
                self.method_index = move_index(self.method_index, length, direction);
            }
            Tab::Data => {
                self.artifact_index =
                    move_index(self.artifact_index, self.artifacts.len(), direction);
                self.clear_data_view();
            }
        }
    }

    async fn open_selected(&mut self) {
        if self.focus == Focus::Models {
            self.focus = Focus::Content;
            return;
        }
        match self.tab {
            Tab::Overview => {}
            Tab::Methods => {
                if self.run_receiver.is_some() {
                    self.error = Some("Only one method can run at a time".to_owned());
                    return;
                }
                if let Some(method) = self.selected_method().cloned() {
                    self.form = Some(MethodForm {
                        mode: form_for(&method.arguments),
                        method,
                        selected_field: 0,
                    });
                    self.mode = InputMode::MethodForm;
                }
            }
            Tab::Data => {
                if self.content.is_some() && !self.versions.is_empty() {
                    self.load_cursor_version().await;
                } else {
                    self.request_latest().await;
                }
            }
        }
    }

    async fn review_form(&mut self) {
        let Some(form) = self.form.as_ref() else {
            return;
        };
        match build_payload(&form.mode, &form.method.arguments) {
            Ok(payload) => {
                let Some(model) = self.selected_model().map(|model| model.name.clone()) else {
                    return;
                };
                self.status = "Validating method…".to_owned();
                match self.client.validate_method(&model, &form.method.name).await {
                    Ok(()) => {
                        self.pending_payload = Some(payload);
                        self.status = "Review the method invocation".to_owned();
                        self.mode = InputMode::Review;
                    }
                    Err(error) => self.fail(error.to_string()),
                }
            }
            Err(error) => self.fail(error.to_string()),
        }
    }

    pub fn redacted_pending(&self) -> Option<Value> {
        let form = self.form.as_ref()?;
        let payload = self.pending_payload.as_ref()?;
        Some(redacted_payload(payload, &form.method.arguments))
    }

    async fn execute_pending(&mut self) {
        let Some(selected_model) = self.selected_model().cloned() else {
            return;
        };
        let model = selected_model.name.clone();
        let Some(method) = self.form.as_ref().map(|form| form.method.name.clone()) else {
            return;
        };
        if is_destructive_method(&method) {
            match self.client.model(&model).await {
                Ok(current) if current.id == selected_model.id && current.name == model => {}
                Ok(_) => {
                    self.fail(
                        "The model changed after review; refresh and review again".to_owned(),
                    );
                    return;
                }
                Err(error) => {
                    self.fail(format!(
                        "Could not re-check the destructive target: {error}"
                    ));
                    return;
                }
            }
        }
        let Some(payload) = self.pending_payload.take() else {
            return;
        };
        match self.client.run_method(&model, &method, &payload).await {
            Ok(receiver) => {
                self.run_logs.clear();
                self.run_receiver = Some(receiver);
                self.running_model = Some(model);
                self.form = None;
                self.mode = InputMode::Normal;
                self.status = format!("Running {method}…");
            }
            Err(error) => self.fail(error.to_string()),
        }
    }

    async fn cancel_run(&mut self) {
        let Some(model) = self.running_model.clone() else {
            return;
        };
        match self.client.cancel_method(&model).await {
            Ok(()) => self.status = "Cancellation requested".to_owned(),
            Err(error) => self.fail(error.to_string()),
        }
    }

    async fn request_latest(&mut self) {
        let Some(artifact) = self.selected_artifact() else {
            return;
        };
        if artifact.size > self.config.preview_limit {
            self.mode = InputMode::LargeConfirm(LargeAction::Latest);
        } else {
            self.load_latest().await;
        }
    }

    async fn load_latest(&mut self) {
        let Some(model) = self.selected_model().map(|model| model.name.clone()) else {
            return;
        };
        let Some(name) = self.selected_artifact().map(|item| item.name.clone()) else {
            return;
        };
        let (content, versions) = tokio::join!(
            self.client.latest_data(&model, &name),
            self.client.data_versions(&model, &name),
        );
        match content {
            Ok(value) => self.content = Some(value),
            Err(error) => self.fail(error.to_string()),
        }
        match versions {
            Ok(value) => self.versions = value,
            Err(error) => self.fail(error.to_string()),
        }
        self.version_cursor = 0;
        self.diff = None;
        if self.error.is_none() {
            self.status = format!("Loaded {name}");
        }
    }

    fn move_version(&mut self, direction: isize) {
        self.version_cursor = move_index(self.version_cursor, self.versions.len(), direction);
    }

    async fn load_cursor_version(&mut self) {
        let Some(version) = self.versions.get(self.version_cursor) else {
            return;
        };
        if version.size > self.config.preview_limit {
            self.mode = InputMode::LargeConfirm(LargeAction::Version(version.version));
        } else {
            self.load_version(version.version).await;
        }
    }

    async fn load_version(&mut self, version: u64) {
        let Some(model) = self.selected_model().map(|model| model.name.clone()) else {
            return;
        };
        let Some(name) = self.selected_artifact().map(|item| item.name.clone()) else {
            return;
        };
        match self.client.data_version(&model, &name, version).await {
            Ok(content) => {
                self.content = Some(content);
                self.diff = None;
                self.status = format!("Loaded {name} v{version}");
            }
            Err(error) => self.fail(error.to_string()),
        }
    }

    async fn compare_versions(&mut self) {
        let (Some(a_index), Some(b_index)) = (self.compare_a, self.compare_b) else {
            return;
        };
        let (Some(a), Some(b)) = (self.versions.get(a_index), self.versions.get(b_index)) else {
            return;
        };
        if a.size > self.config.preview_limit || b.size > self.config.preview_limit {
            self.mode = InputMode::LargeConfirm(LargeAction::Diff(a.version, b.version));
        } else {
            self.load_diff(a.version, b.version).await;
        }
    }

    async fn load_diff(&mut self, a_version: u64, b_version: u64) {
        let Some(model) = self.selected_model().map(|model| model.name.clone()) else {
            return;
        };
        let Some(artifact) = self.selected_artifact().cloned() else {
            return;
        };
        let (a, b) = tokio::join!(
            self.client.data_version(&model, &artifact.name, a_version),
            self.client.data_version(&model, &artifact.name, b_version),
        );
        match (a, b) {
            (Ok(a), Ok(b)) => {
                self.diff = if artifact.content_type == "application/json"
                    || (!a.content.is_string() && !b.content.is_string())
                {
                    Some(json_diff(&a.content, &b.content))
                } else if let (Some(left), Some(right)) = (a.content.as_str(), b.content.as_str()) {
                    Some(text_diff(
                        &format!("v{a_version}"),
                        left,
                        &format!("v{b_version}"),
                        right,
                    ))
                } else {
                    None
                };
                self.status = if self.diff.is_some() {
                    format!("Comparing v{a_version} with v{b_version}")
                } else {
                    "Binary content cannot be compared".to_owned()
                };
            }
            (Err(error), _) | (_, Err(error)) => self.fail(error.to_string()),
        }
    }

    async fn perform_large_action(&mut self, action: LargeAction) {
        match action {
            LargeAction::Latest => self.load_latest().await,
            LargeAction::Version(version) => self.load_version(version).await,
            LargeAction::Diff(a, b) => self.load_diff(a, b).await,
        }
    }

    fn apply_filter(&mut self) {
        let needle = self.search.to_ascii_lowercase();
        self.filtered_models = self
            .models
            .iter()
            .enumerate()
            .filter(|(_, model)| {
                needle.is_empty()
                    || model.name.to_ascii_lowercase().contains(&needle)
                    || model.model_type.to_ascii_lowercase().contains(&needle)
            })
            .map(|(index, _)| index)
            .collect();
        self.model_index = self
            .model_index
            .min(self.filtered_models.len().saturating_sub(1));
    }

    fn clear_data_view(&mut self) {
        self.content = None;
        self.versions.clear();
        self.version_cursor = 0;
        self.compare_a = None;
        self.compare_b = None;
        self.diff = None;
    }

    fn fail(&mut self, message: String) {
        self.status = "Operation failed".to_owned();
        self.error = Some(message);
    }
}

fn move_index(current: usize, length: usize, direction: isize) -> usize {
    if length == 0 {
        return 0;
    }
    if direction < 0 {
        current.saturating_sub(direction.unsigned_abs())
    } else {
        (current + direction as usize).min(length - 1)
    }
}

fn cycle_field(field: &mut crate::schema::FormField) {
    match &field.kind {
        FieldKind::Boolean => {
            field.value = if field.value == "true" {
                "false"
            } else {
                "true"
            }
            .to_owned();
        }
        FieldKind::Enum(values) if !values.is_empty() => {
            let index = values
                .iter()
                .position(|value| value == &field.value)
                .unwrap_or(0);
            field.value = values[(index + 1) % values.len()].clone();
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Mutex};

    use async_trait::async_trait;
    use crossterm::event::KeyEvent;
    use serde_json::json;
    use tokio::sync::mpsc;

    use crate::{
        config::DEFAULT_PREVIEW_LIMIT,
        error::Result,
        swamp::{DataContent, ModelDetails, TypeDescription, TypeName},
    };

    use super::*;

    #[test]
    fn selection_is_bounded() {
        assert_eq!(move_index(0, 3, -1), 0);
        assert_eq!(move_index(1, 3, 1), 2);
        assert_eq!(move_index(2, 3, 1), 2);
        assert_eq!(move_index(2, 0, -1), 0);
    }

    #[tokio::test]
    async fn model_method_and_data_flow() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let client = Arc::new(MockClient {
            calls: Arc::clone(&calls),
        });
        let config = Config {
            repo_dir: PathBuf::from("/repo"),
            swamp_bin: PathBuf::from("swamp"),
            preview_limit: DEFAULT_PREVIEW_LIMIT,
        };
        let mut app = App::new(config, client);
        app.load().await;
        assert_eq!(app.selected_model().unwrap().name, "hello-world");
        assert_eq!(app.artifacts.len(), 1);

        app.focus = Focus::Content;
        app.tab = Tab::Methods;
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .await;
        let FormMode::Fields(fields) = &mut app.form.as_mut().unwrap().mode else {
            panic!("expected form fields")
        };
        fields[0].value = "echo hello".to_owned();
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .await;
        assert!(matches!(app.mode, InputMode::Review));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .await;
        app.tick().await;
        assert!(calls.lock().unwrap().contains(&"run".to_owned()));

        app.tab = Tab::Data;
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .await;
        assert_eq!(app.versions.len(), 2);
        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE))
            .await;
        app.handle_key(KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE))
            .await;
        app.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE))
            .await;
        assert!(
            app.diff
                .as_deref()
                .is_some_and(|diff| diff.contains("$.value"))
        );
    }

    struct MockClient {
        calls: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl SwampClient for MockClient {
        async fn version(&self) -> Result<String> {
            Ok("swamp test".to_owned())
        }

        async fn models(&self) -> Result<Vec<ModelSummary>> {
            Ok(vec![ModelSummary {
                id: "model-id".to_owned(),
                name: "hello-world".to_owned(),
                model_type: "command/shell".to_owned(),
            }])
        }

        async fn model(&self, _name: &str) -> Result<ModelDetails> {
            Ok(ModelDetails {
                id: "model-id".to_owned(),
                name: "hello-world".to_owned(),
                model_type: "command/shell".to_owned(),
                version: Some(1),
                type_version: Some("1".to_owned()),
                tags: json!({}),
                global_arguments: json!({}),
                methods: vec![method()],
            })
        }

        async fn describe_type(&self, _model_type: &str) -> Result<TypeDescription> {
            Ok(TypeDescription {
                model_type: TypeName {
                    raw: "command/shell".to_owned(),
                    normalized: "command/shell".to_owned(),
                },
                version: "1".to_owned(),
                methods: vec![method()],
                data_output_specs: Vec::new(),
            })
        }

        async fn validate_method(&self, _model: &str, _method: &str) -> Result<()> {
            self.calls.lock().unwrap().push("validate".to_owned());
            Ok(())
        }

        async fn run_method(
            &self,
            _model: &str,
            _method: &str,
            _input: &Value,
        ) -> Result<mpsc::UnboundedReceiver<RunEvent>> {
            self.calls.lock().unwrap().push("run".to_owned());
            let (sender, receiver) = mpsc::unbounded_channel();
            sender
                .send(RunEvent::Finished {
                    success: true,
                    lock_contended: false,
                    message: "done".to_owned(),
                })
                .unwrap();
            Ok(receiver)
        }

        async fn cancel_method(&self, _model: &str) -> Result<()> {
            Ok(())
        }

        async fn data(&self, _model: &str) -> Result<Vec<DataArtifact>> {
            Ok(vec![DataArtifact {
                id: "data-id".to_owned(),
                name: "result".to_owned(),
                version: 2,
                content_type: "application/json".to_owned(),
                data_type: "resource".to_owned(),
                streaming: false,
                size: 10,
                created_at: "now".to_owned(),
                lifetime: "infinite".to_owned(),
                owner_type: "model-method".to_owned(),
                tags: Default::default(),
            }])
        }

        async fn latest_data(&self, _model: &str, _name: &str) -> Result<DataContent> {
            Ok(content(2))
        }

        async fn data_versions(&self, _model: &str, _name: &str) -> Result<Vec<DataVersion>> {
            Ok(vec![version(2), version(1)])
        }

        async fn data_version(
            &self,
            _model: &str,
            _name: &str,
            version: u64,
        ) -> Result<DataContent> {
            Ok(content(version))
        }
    }

    fn method() -> MethodSpec {
        MethodSpec {
            name: "execute".to_owned(),
            description: "Execute".to_owned(),
            arguments: json!({
                "type": "object",
                "properties": {"run": {"type": "string", "minLength": 1}},
                "required": ["run"],
                "additionalProperties": false
            }),
        }
    }

    fn version(number: u64) -> DataVersion {
        DataVersion {
            version: number,
            created_at: "now".to_owned(),
            size: 10,
            checksum: String::new(),
            is_latest: number == 2,
        }
    }

    fn content(version: u64) -> DataContent {
        DataContent {
            id: format!("data-{version}"),
            name: "result".to_owned(),
            model_name: "hello-world".to_owned(),
            version,
            content_type: "application/json".to_owned(),
            size: 10,
            created_at: "now".to_owned(),
            lifetime: "infinite".to_owned(),
            streaming: false,
            tags: Default::default(),
            owner_type: "model-method".to_owned(),
            owner_definition: None,
            content: json!({"value": version}),
        }
    }
}
