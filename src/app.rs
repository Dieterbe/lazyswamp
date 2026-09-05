use std::{
    collections::{BTreeSet, HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::Value;
use tokio::{
    sync::{Semaphore, mpsc},
    task::{JoinHandle, JoinSet},
};

use crate::{
    config::Config,
    diff::{display_content, json_diff, text_diff},
    schema::{
        FieldKind, FormMode, build_payload, form_for, is_destructive_method, redacted_payload,
    },
    swamp::{
        DataArtifact, DataContent, DataVersion, MethodSpec, ModelDetails, ModelSummary, RunEvent,
        SwampClient, TypeDescription, WorkflowDefinition, WorkflowNode, WorkflowSummary,
    },
};

type StartupOutcome = (
    crate::error::Result<String>,
    crate::error::Result<Vec<ModelSummary>>,
    crate::error::Result<Vec<DataArtifact>>,
    crate::error::Result<Vec<WorkflowSummary>>,
);
type StartupTask = JoinHandle<StartupOutcome>;

enum PreloadEvent {
    Type(String, crate::error::Result<TypeDescription>),
    Workflow(String, crate::error::Result<WorkflowDefinition>),
}

enum PreloadRequest {
    Type(String),
    Workflow(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Overview,
    Data,
    Workflows,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Models,
    Content,
    Outputs,
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
    pub workflows: Vec<WorkflowSummary>,
    pub filtered_workflows: Vec<usize>,
    pub workflow_index: usize,
    pub workflow: Option<WorkflowDefinition>,
    pub workflow_node_index: usize,
    pub detail: Option<ModelDetails>,
    pub type_description: Option<TypeDescription>,
    pub method_index: usize,
    pub output_index: usize,
    pub expanded_output: Option<usize>,
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
    pub run_log_model: Option<String>,
    pub run_log_method: Option<String>,
    pub run_log_visible: bool,
    pub should_quit: bool,
    refresh_after_run: bool,
    model_cache: HashMap<String, ModelDetails>,
    type_cache: HashMap<String, TypeDescription>,
    data_cache: HashMap<String, Vec<DataArtifact>>,
    workflow_cache: HashMap<String, WorkflowDefinition>,
    startup_task: Option<StartupTask>,
    model_task: Option<(String, JoinHandle<crate::error::Result<ModelDetails>>)>,
    type_task: Option<(String, JoinHandle<crate::error::Result<TypeDescription>>)>,
    data_task: Option<(String, JoinHandle<crate::error::Result<Vec<DataArtifact>>>)>,
    workflow_task: Option<(String, JoinHandle<crate::error::Result<WorkflowDefinition>>)>,
    preload_task: Option<JoinHandle<()>>,
    preload_receiver: Option<mpsc::UnboundedReceiver<PreloadEvent>>,
    preloading_types: HashSet<String>,
    preloading_workflows: HashSet<String>,
    preload_total: usize,
    preload_complete: usize,
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
            workflows: Vec::new(),
            filtered_workflows: Vec::new(),
            workflow_index: 0,
            workflow: None,
            workflow_node_index: 0,
            detail: None,
            type_description: None,
            method_index: 0,
            output_index: 0,
            expanded_output: None,
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
            run_log_model: None,
            run_log_method: None,
            run_log_visible: false,
            should_quit: false,
            refresh_after_run: false,
            model_cache: HashMap::new(),
            type_cache: HashMap::new(),
            data_cache: HashMap::new(),
            workflow_cache: HashMap::new(),
            startup_task: None,
            model_task: None,
            type_task: None,
            data_task: None,
            workflow_task: None,
            preload_task: None,
            preload_receiver: None,
            preloading_types: HashSet::new(),
            preloading_workflows: HashSet::new(),
            preload_total: 0,
            preload_complete: 0,
        }
    }

    pub fn begin_load(&mut self) {
        if let Some(task) = self.startup_task.take() {
            task.abort();
        }
        if let Some((_, task)) = self.model_task.take() {
            task.abort();
        }
        if let Some((_, task)) = self.type_task.take() {
            task.abort();
        }
        if let Some((_, task)) = self.data_task.take() {
            task.abort();
        }
        if let Some((_, task)) = self.workflow_task.take() {
            task.abort();
        }
        if let Some(task) = self.preload_task.take() {
            task.abort();
        }
        self.preload_receiver = None;
        self.preloading_types.clear();
        self.preloading_workflows.clear();
        let client = Arc::clone(&self.client);
        self.status = "Loading repository…".to_owned();
        self.startup_task = Some(tokio::spawn(async move {
            tokio::join!(
                client.version(),
                client.models(),
                client.all_data(),
                client.workflows()
            )
        }));
    }

    pub async fn load(&mut self) {
        let (version, models, all_data, workflows) = tokio::join!(
            self.client.version(),
            self.client.models(),
            self.client.all_data(),
            self.client.workflows()
        );
        match version {
            Ok(version) => self.swamp_version = version,
            Err(error) => self.fail(error.to_string()),
        }
        match models {
            Ok(models) => {
                self.models = models;
                self.apply_filter();
                self.load_selected_model().await;
            }
            Err(error) => self.fail(error.to_string()),
        }
        if let Ok(artifacts) = all_data {
            self.cache_all_data(artifacts);
        }
        if let Ok(workflows) = workflows {
            self.workflows = workflows;
            self.apply_filter();
            self.load_selected_workflow().await;
        }
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

    pub fn visible_workflows(&self) -> impl Iterator<Item = &WorkflowSummary> {
        self.filtered_workflows
            .iter()
            .filter_map(|index| self.workflows.get(*index))
    }

    pub fn selected_workflow(&self) -> Option<&WorkflowSummary> {
        self.filtered_workflows
            .get(self.workflow_index)
            .and_then(|index| self.workflows.get(*index))
    }

    pub fn workflow_nodes(&self) -> Vec<WorkflowNode<'_>> {
        self.workflow
            .as_ref()
            .map(WorkflowDefinition::nodes)
            .unwrap_or_default()
    }

    pub fn selected_workflow_node(&self) -> Option<WorkflowNode<'_>> {
        self.workflow_nodes().get(self.workflow_node_index).copied()
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
            .or_else(|| {
                self.selected_model()
                    .and_then(|model| model.methods.get(self.method_index))
            })
    }

    pub fn selected_artifact(&self) -> Option<&DataArtifact> {
        self.artifacts.get(self.artifact_index)
    }

    pub fn run_log_matches_selection(&self) -> bool {
        self.run_log_model.as_deref() == self.selected_model().map(|model| model.name.as_str())
            && self.run_log_method.as_deref()
                == self.selected_method().map(|method| method.name.as_str())
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
        self.poll_background().await;
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
            self.schedule_data(true);
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
            KeyCode::Esc if self.run_log_visible && self.run_log_matches_selection() => {
                self.run_log_visible = false
            }
            KeyCode::Char('?') => self.mode = InputMode::Help,
            KeyCode::Char('/') if self.focus == Focus::Models => {
                self.search.clear();
                self.mode = InputMode::Search;
            }
            KeyCode::Char('1') => self.activate_tab(Tab::Overview),
            KeyCode::Char('2') => self.activate_tab(Tab::Data),
            KeyCode::Char('3') => self.activate_tab(Tab::Workflows),
            KeyCode::Tab | KeyCode::BackTab => {
                self.focus = match self.focus {
                    Focus::Models => Focus::Content,
                    Focus::Content if self.tab == Tab::Overview => Focus::Outputs,
                    Focus::Content => Focus::Models,
                    Focus::Outputs => Focus::Models,
                }
            }
            KeyCode::Char('r') => {
                if self.focus == Focus::Models {
                    self.model_cache.clear();
                    self.type_cache.clear();
                    self.data_cache.clear();
                    self.workflow_cache.clear();
                    self.begin_load();
                } else {
                    self.refresh_current();
                }
            }
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1).await,
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1).await,
            KeyCode::Enter => self.open_selected().await,
            KeyCode::Char('c') if self.run_receiver.is_some() => self.cancel_run().await,
            KeyCode::Char(' ') if self.tab == Tab::Overview && self.focus == Focus::Outputs => {
                self.toggle_output()
            }
            KeyCode::Char('[') if self.tab == Tab::Overview && self.focus == Focus::Outputs => {
                self.move_output(-1)
            }
            KeyCode::Char(']') if self.tab == Tab::Overview && self.focus == Focus::Outputs => {
                self.move_output(1)
            }
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
                if self.tab == Tab::Workflows {
                    self.workflow_index = 0;
                    self.workflow_selection_changed();
                } else {
                    self.model_index = 0;
                    self.selection_changed();
                }
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

    fn activate_tab(&mut self, tab: Tab) {
        self.tab = tab;
        if tab != Tab::Overview && self.focus == Focus::Outputs {
            self.focus = Focus::Content;
        }
        match tab {
            Tab::Overview => {
                abort_named_task(&mut self.data_task);
                self.schedule_model(false);
                self.schedule_type(false);
            }
            Tab::Data => {
                abort_named_task(&mut self.type_task);
                self.schedule_data(false);
            }
            Tab::Workflows => {
                abort_named_task(&mut self.model_task);
                abort_named_task(&mut self.type_task);
                abort_named_task(&mut self.data_task);
                self.schedule_workflow(false);
            }
        }
    }

    fn selection_changed(&mut self) {
        self.method_index = 0;
        self.output_index = 0;
        self.expanded_output = None;
        self.artifact_index = 0;
        self.clear_data_view();

        let Some(model) = self.selected_model().cloned() else {
            self.detail = None;
            self.type_description = None;
            self.artifacts.clear();
            self.status = "No models found".to_owned();
            return;
        };
        self.detail = self.model_cache.get(&model.name).cloned();
        self.type_description = self.type_cache.get(&model.model_type).cloned();
        self.artifacts = self
            .data_cache
            .get(&model.name)
            .cloned()
            .unwrap_or_default();
        self.schedule_model(false);
        match self.tab {
            Tab::Overview => {
                abort_named_task(&mut self.data_task);
                self.schedule_type(false);
            }
            Tab::Data => {
                abort_named_task(&mut self.type_task);
                self.schedule_data(false);
            }
            Tab::Workflows => {}
        }
    }

    fn workflow_selection_changed(&mut self) {
        self.workflow_node_index = 0;
        let Some(workflow) = self.selected_workflow().cloned() else {
            self.workflow = None;
            self.status = "No workflows found".to_owned();
            return;
        };
        self.workflow = self.workflow_cache.get(&workflow.name).cloned();
        self.schedule_workflow(false);
    }

    fn cache_all_data(&mut self, artifacts: Vec<DataArtifact>) {
        self.data_cache.clear();
        for model in &self.models {
            self.data_cache.entry(model.name.clone()).or_default();
        }
        for mut artifact in artifacts {
            let model_name = if artifact.model_name.is_empty() {
                self.models
                    .iter()
                    .find(|model| model.id == artifact.model_id)
                    .map(|model| model.name.clone())
            } else {
                Some(artifact.model_name.clone())
            };
            if let Some(model_name) = model_name {
                artifact.model_name.clone_from(&model_name);
                self.data_cache
                    .entry(model_name)
                    .or_default()
                    .push(artifact);
            }
        }
        for artifacts in self.data_cache.values_mut() {
            artifacts.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        }
    }

    fn start_preloading(&mut self) {
        if let Some(task) = self.preload_task.take() {
            task.abort();
        }
        self.preload_receiver = None;

        let model_types: Vec<String> = self
            .models
            .iter()
            .map(|model| model.model_type.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let mut workflow_names: Vec<String> = self
            .workflows
            .iter()
            .map(|workflow| workflow.name.clone())
            .collect();

        self.preloading_types = model_types.iter().cloned().collect();
        self.preloading_workflows = workflow_names.iter().cloned().collect();
        self.preload_total = model_types.len() + workflow_names.len();
        self.preload_complete = 0;
        if self.preload_total == 0 {
            return;
        }

        let workers = std::thread::available_parallelism()
            .map_or(4, usize::from)
            .clamp(4, 12)
            .min(self.preload_total);
        let semaphore = Arc::new(Semaphore::new(workers));
        let client = Arc::clone(&self.client);
        let (sender, receiver) = mpsc::unbounded_channel();
        let mut requests = Vec::with_capacity(self.preload_total);
        if !workflow_names.is_empty() {
            requests.push(PreloadRequest::Workflow(workflow_names.remove(0)));
        }
        requests.extend(workflow_names.into_iter().map(PreloadRequest::Workflow));
        requests.extend(model_types.into_iter().map(PreloadRequest::Type));
        self.preload_receiver = Some(receiver);
        self.status = format!("Preloading 0/{}…", self.preload_total);
        self.preload_task = Some(tokio::spawn(async move {
            let mut tasks = JoinSet::new();
            for request in requests {
                let client = Arc::clone(&client);
                let semaphore = Arc::clone(&semaphore);
                let sender = sender.clone();
                tasks.spawn(async move {
                    let Ok(_permit) = semaphore.acquire_owned().await else {
                        return;
                    };
                    match request {
                        PreloadRequest::Type(model_type) => {
                            let result = client.describe_type(&model_type).await;
                            let _ = sender.send(PreloadEvent::Type(model_type, result));
                        }
                        PreloadRequest::Workflow(name) => {
                            let result = client.workflow(&name).await;
                            let _ = sender.send(PreloadEvent::Workflow(name, result));
                        }
                    };
                });
            }
            while tasks.join_next().await.is_some() {}
        }));
    }

    fn schedule_model(&mut self, force: bool) {
        let Some(model) = self.selected_model().cloned() else {
            return;
        };
        if !force && self.model_cache.contains_key(&model.name) {
            self.detail = self.model_cache.get(&model.name).cloned();
            return;
        }
        if let Some((_, task)) = self.model_task.take() {
            task.abort();
        }
        let client = Arc::clone(&self.client);
        let name = model.name.clone();
        let task_name = name.clone();
        let delay = if force { 0 } else { 120 };
        self.status = format!("Loading {name}…");
        self.model_task = Some((
            name,
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(delay)).await;
                client.model(&task_name).await
            }),
        ));
    }

    fn schedule_type(&mut self, force: bool) {
        let Some(model) = self.selected_model().cloned() else {
            return;
        };
        if !force && self.type_cache.contains_key(&model.model_type) {
            self.type_description = self.type_cache.get(&model.model_type).cloned();
            return;
        }
        if !force && self.preloading_types.contains(&model.model_type) {
            return;
        }
        if let Some((_, task)) = self.type_task.take() {
            task.abort();
        }
        let client = Arc::clone(&self.client);
        let model_type = model.model_type.clone();
        let task_type = model_type.clone();
        let delay = if force { 0 } else { 120 };
        self.type_task = Some((
            model_type,
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(delay)).await;
                client.describe_type(&task_type).await
            }),
        ));
    }

    fn schedule_data(&mut self, force: bool) {
        let Some(model) = self.selected_model().cloned() else {
            return;
        };
        if !force && self.data_cache.contains_key(&model.name) {
            self.artifacts = self
                .data_cache
                .get(&model.name)
                .cloned()
                .unwrap_or_default();
            return;
        }
        if let Some((_, task)) = self.data_task.take() {
            task.abort();
        }
        let client = Arc::clone(&self.client);
        let name = model.name.clone();
        let task_name = name.clone();
        let delay = if force { 0 } else { 120 };
        self.status = format!("Loading data for {name}…");
        self.data_task = Some((
            name,
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(delay)).await;
                client.data(&task_name).await
            }),
        ));
    }

    fn schedule_workflow(&mut self, force: bool) {
        let Some(workflow) = self.selected_workflow().cloned() else {
            return;
        };
        if !force && self.workflow_cache.contains_key(&workflow.name) {
            self.workflow = self.workflow_cache.get(&workflow.name).cloned();
            return;
        }
        if !force && self.preloading_workflows.contains(&workflow.name) {
            self.status = format!("Preloading workflow {}…", workflow.name);
            return;
        }
        if let Some((_, task)) = self.workflow_task.take() {
            task.abort();
        }
        let client = Arc::clone(&self.client);
        let name = workflow.name.clone();
        let task_name = name.clone();
        let delay = if force { 0 } else { 120 };
        self.status = format!("Loading workflow {name}…");
        self.workflow_task = Some((
            name,
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(delay)).await;
                client.workflow(&task_name).await
            }),
        ));
    }

    async fn poll_background(&mut self) {
        if self
            .startup_task
            .as_ref()
            .is_some_and(|task| task.is_finished())
        {
            let task = self.startup_task.take().expect("checked above");
            match task.await {
                Ok((version, models, all_data, workflows)) => {
                    match version {
                        Ok(version) => self.swamp_version = version,
                        Err(error) => self.fail(error.to_string()),
                    }
                    let mut models_loaded = false;
                    match models {
                        Ok(models) => {
                            self.models = models;
                            self.apply_filter();
                            self.model_index = self
                                .model_index
                                .min(self.filtered_models.len().saturating_sub(1));
                            models_loaded = true;
                        }
                        Err(error) => self.fail(error.to_string()),
                    }
                    match all_data {
                        Ok(artifacts) if models_loaded => self.cache_all_data(artifacts),
                        Ok(_) => {}
                        Err(error) => self.error = Some(error.to_string()),
                    }
                    match workflows {
                        Ok(workflows) => {
                            self.workflows = workflows;
                            self.apply_filter();
                            self.workflow_index = self
                                .workflow_index
                                .min(self.filtered_workflows.len().saturating_sub(1));
                        }
                        Err(error) => self.error = Some(error.to_string()),
                    }
                    if models_loaded {
                        self.start_preloading();
                        self.selection_changed();
                        if self.tab == Tab::Workflows {
                            self.workflow_selection_changed();
                        }
                    }
                }
                Err(error) => self.fail(format!("Startup task failed: {error}")),
            }
        }

        if self
            .model_task
            .as_ref()
            .is_some_and(|(_, task)| task.is_finished())
        {
            let (name, task) = self.model_task.take().expect("checked above");
            match task.await {
                Ok(Ok(detail)) => {
                    self.model_cache.insert(name.clone(), detail.clone());
                    if self
                        .selected_model()
                        .is_some_and(|model| model.name == name)
                    {
                        self.detail = Some(detail);
                        self.status = format!("Loaded {name}");
                    }
                }
                Ok(Err(error)) => {
                    if self
                        .selected_model()
                        .is_some_and(|model| model.name == name)
                    {
                        self.fail(error.to_string());
                    }
                }
                Err(error) if !error.is_cancelled() => {
                    self.fail(format!("Model loading task failed: {error}"));
                }
                Err(_) => {}
            }
        }

        if self
            .type_task
            .as_ref()
            .is_some_and(|(_, task)| task.is_finished())
        {
            let (model_type, task) = self.type_task.take().expect("checked above");
            match task.await {
                Ok(Ok(description)) => {
                    self.type_cache
                        .insert(model_type.clone(), description.clone());
                    if self
                        .selected_model()
                        .is_some_and(|model| model.model_type == model_type)
                    {
                        self.type_description = Some(description);
                    }
                }
                Ok(Err(error)) => {
                    if self
                        .selected_model()
                        .is_some_and(|model| model.model_type == model_type)
                    {
                        self.fail(error.to_string());
                    }
                }
                Err(error) if !error.is_cancelled() => {
                    self.fail(format!("Type loading task failed: {error}"));
                }
                Err(_) => {}
            }
        }

        if self
            .data_task
            .as_ref()
            .is_some_and(|(_, task)| task.is_finished())
        {
            let (name, task) = self.data_task.take().expect("checked above");
            match task.await {
                Ok(Ok(artifacts)) => {
                    self.data_cache.insert(name.clone(), artifacts.clone());
                    if self
                        .selected_model()
                        .is_some_and(|model| model.name == name)
                    {
                        self.artifacts = artifacts;
                        self.artifact_index = self
                            .artifact_index
                            .min(self.artifacts.len().saturating_sub(1));
                        self.status = format!("Loaded data for {name}");
                    }
                }
                Ok(Err(error)) => {
                    if self
                        .selected_model()
                        .is_some_and(|model| model.name == name)
                    {
                        self.fail(error.to_string());
                    }
                }
                Err(error) if !error.is_cancelled() => {
                    self.fail(format!("Data loading task failed: {error}"));
                }
                Err(_) => {}
            }
        }

        if self
            .workflow_task
            .as_ref()
            .is_some_and(|(_, task)| task.is_finished())
        {
            let (name, task) = self.workflow_task.take().expect("checked above");
            match task.await {
                Ok(Ok(workflow)) => {
                    self.workflow_cache.insert(name.clone(), workflow.clone());
                    if self
                        .selected_workflow()
                        .is_some_and(|selected| selected.name == name)
                    {
                        self.workflow = Some(workflow);
                        self.workflow_node_index = self
                            .workflow_node_index
                            .min(self.workflow_nodes().len().saturating_sub(1));
                        self.status = format!("Loaded workflow {name}");
                    }
                }
                Ok(Err(error)) => {
                    if self
                        .selected_workflow()
                        .is_some_and(|selected| selected.name == name)
                    {
                        self.fail(error.to_string());
                    }
                }
                Err(error) if !error.is_cancelled() => {
                    self.fail(format!("Workflow loading task failed: {error}"));
                }
                Err(_) => {}
            }
        }

        let mut preload_events = Vec::new();
        if let Some(receiver) = self.preload_receiver.as_mut() {
            while let Ok(event) = receiver.try_recv() {
                preload_events.push(event);
            }
        }
        for event in preload_events {
            self.preload_complete += 1;
            match event {
                PreloadEvent::Type(model_type, result) => {
                    self.preloading_types.remove(&model_type);
                    if let Ok(description) = result {
                        self.type_cache
                            .insert(model_type.clone(), description.clone());
                        if self
                            .selected_model()
                            .is_some_and(|model| model.model_type == model_type)
                        {
                            self.type_description = Some(description);
                        }
                    }
                    // Type descriptions only enrich output schemas; model details already
                    // contain the methods needed for browsing and execution. Missing extension
                    // types stay silent here, and opening Overview can retry them explicitly.
                }
                PreloadEvent::Workflow(name, result) => {
                    self.preloading_workflows.remove(&name);
                    match result {
                        Ok(workflow) => {
                            self.workflow_cache.insert(name.clone(), workflow.clone());
                            if self
                                .selected_workflow()
                                .is_some_and(|selected| selected.name == name)
                            {
                                self.workflow = Some(workflow);
                                self.workflow_node_index = self
                                    .workflow_node_index
                                    .min(self.workflow_nodes().len().saturating_sub(1));
                            }
                        }
                        Err(error) => {
                            if self.tab == Tab::Workflows
                                && self
                                    .selected_workflow()
                                    .is_some_and(|selected| selected.name == name)
                            {
                                self.error = Some(error.to_string());
                            }
                        }
                    }
                }
            }
            self.status = if self.preload_complete == self.preload_total {
                format!(
                    "Preloaded {} model(s), {} workflow(s)",
                    self.models.len(),
                    self.workflows.len()
                )
            } else {
                format!(
                    "Preloading {}/{}…",
                    self.preload_complete, self.preload_total
                )
            };
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
        self.output_index = 0;
        self.expanded_output = None;
        self.artifact_index = 0;
        self.clear_data_view();
        if self.error.is_none() {
            self.status = format!("Loaded {}", model.name);
        }
    }

    async fn load_selected_workflow(&mut self) {
        let Some(workflow) = self.selected_workflow().cloned() else {
            self.workflow = None;
            return;
        };
        match self.client.workflow(&workflow.name).await {
            Ok(definition) => {
                self.workflow_cache
                    .insert(workflow.name.clone(), definition.clone());
                self.workflow = Some(definition);
            }
            Err(error) => self.fail(error.to_string()),
        }
    }

    fn refresh_current(&mut self) {
        match self.tab {
            Tab::Overview => {
                self.schedule_model(true);
                self.schedule_type(true);
            }
            Tab::Data => self.schedule_data(true),
            Tab::Workflows => self.schedule_workflow(true),
        }
    }

    async fn move_selection(&mut self, direction: isize) {
        if self.focus == Focus::Models {
            if self.tab == Tab::Workflows {
                let previous = self.workflow_index;
                self.workflow_index = move_index(
                    self.workflow_index,
                    self.filtered_workflows.len(),
                    direction,
                );
                if previous != self.workflow_index {
                    self.workflow_selection_changed();
                }
                return;
            }
            let previous = self.model_index;
            self.model_index = move_index(self.model_index, self.filtered_models.len(), direction);
            if previous != self.model_index {
                self.selection_changed();
            }
            return;
        }
        match self.tab {
            Tab::Overview => {
                if self.focus == Focus::Outputs {
                    self.move_output(direction);
                    return;
                }
                let length = self
                    .type_description
                    .as_ref()
                    .map(|description| description.methods.len())
                    .or_else(|| self.detail.as_ref().map(|detail| detail.methods.len()))
                    .or_else(|| self.selected_model().map(|model| model.methods.len()))
                    .unwrap_or(0);
                let previous = self.method_index;
                self.method_index = move_index(self.method_index, length, direction);
                if previous != self.method_index {
                    self.output_index = 0;
                    self.expanded_output = None;
                }
            }
            Tab::Data => {
                self.artifact_index =
                    move_index(self.artifact_index, self.artifacts.len(), direction);
                self.clear_data_view();
            }
            Tab::Workflows => {
                self.workflow_node_index = move_index(
                    self.workflow_node_index,
                    self.workflow_nodes().len(),
                    direction,
                );
            }
        }
    }

    fn move_output(&mut self, direction: isize) {
        let length = self
            .type_description
            .as_ref()
            .map(|description| description.data_output_specs.len())
            .unwrap_or(0);
        self.output_index = move_index(self.output_index, length, direction);
    }

    fn toggle_output(&mut self) {
        let has_output = self
            .type_description
            .as_ref()
            .is_some_and(|description| self.output_index < description.data_output_specs.len());
        if !has_output {
            return;
        }
        self.expanded_output =
            (self.expanded_output != Some(self.output_index)).then_some(self.output_index);
    }

    async fn open_selected(&mut self) {
        if self.focus == Focus::Models {
            self.focus = Focus::Content;
            return;
        }
        match self.tab {
            Tab::Overview => {
                if self.focus == Focus::Outputs {
                    return;
                }
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
            Tab::Workflows => {}
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
                self.running_model = Some(model.clone());
                self.run_log_model = self.running_model.clone();
                self.run_log_method = Some(method.clone());
                self.run_log_visible = true;
                self.form = None;
                self.mode = InputMode::Normal;
                self.status = format!("Running {model}.{method}…");
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
        self.filtered_workflows = self
            .workflows
            .iter()
            .enumerate()
            .filter(|(_, workflow)| {
                needle.is_empty()
                    || workflow.name.to_ascii_lowercase().contains(&needle)
                    || workflow.description.to_ascii_lowercase().contains(&needle)
            })
            .map(|(index, _)| index)
            .collect();
        self.workflow_index = self
            .workflow_index
            .min(self.filtered_workflows.len().saturating_sub(1));
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

fn abort_named_task<T>(task: &mut Option<(String, JoinHandle<T>)>) {
    if let Some((_, task)) = task.take() {
        task.abort();
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
        app.tab = Tab::Overview;
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
        assert!(app.run_log_matches_selection());
        assert!(app.run_log_visible);
        app.run_logs.push("finished".to_owned());
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .await;
        assert!(!app.run_log_visible);
        app.type_description
            .as_mut()
            .unwrap()
            .methods
            .push(MethodSpec {
                name: "other".to_owned(),
                description: String::new(),
                arguments: json!({}),
            });
        app.method_index = 1;
        assert!(!app.run_log_matches_selection());
        app.run_log_visible = true;
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .await;
        assert!(app.run_log_visible);
        app.method_index = 0;

        app.type_description.as_mut().unwrap().data_output_specs =
            vec![json!({"specName": "first"}), json!({"specName": "second"})];
        app.output_index = 0;
        app.expanded_output = None;
        app.focus = Focus::Outputs;
        app.handle_key(KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE))
            .await;
        assert_eq!(app.output_index, 1);
        app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE))
            .await;
        assert_eq!(app.expanded_output, Some(1));
        app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE))
            .await;
        assert_eq!(app.expanded_output, None);

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

    #[tokio::test]
    async fn background_startup_preloads_models_types_and_data() {
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

        app.begin_load();
        tokio::time::sleep(Duration::from_millis(5)).await;
        app.tick().await;
        assert_eq!(app.models.len(), 2);

        tokio::time::sleep(Duration::from_millis(20)).await;
        app.tick().await;
        assert!(app.selected_method().is_some());
        assert!(app.type_description.is_some());
        assert_eq!(app.artifacts.len(), 1);
        tokio::time::sleep(Duration::from_millis(140)).await;
        app.tick().await;
        assert!(app.detail.is_some());
        let startup_calls = calls.lock().unwrap().clone();
        assert!(startup_calls.contains(&"version".to_owned()));
        assert!(startup_calls.contains(&"models".to_owned()));
        assert_eq!(
            startup_calls
                .iter()
                .filter(|call| call.as_str() == "model")
                .count(),
            1
        );
        assert!(startup_calls.contains(&"describe".to_owned()));
        assert!(startup_calls.contains(&"all_data".to_owned()));
        assert!(startup_calls.contains(&"workflows".to_owned()));
        assert!(startup_calls.contains(&"workflow".to_owned()));
        assert!(!startup_calls.contains(&"data".to_owned()));

        app.activate_tab(Tab::Data);
        assert_eq!(app.artifacts.len(), 1);
        assert!(!calls.lock().unwrap().contains(&"data".to_owned()));

        app.activate_tab(Tab::Workflows);
        assert_eq!(app.workflow_nodes().len(), 5);
        app.focus = Focus::Content;
        app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE))
            .await;
        assert_eq!(app.workflow_node_index, 1);
        assert_eq!(
            app.selected_workflow_node().unwrap().step.name,
            "collect-friend"
        );
    }

    struct MockClient {
        calls: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl SwampClient for MockClient {
        async fn version(&self) -> Result<String> {
            self.calls.lock().unwrap().push("version".to_owned());
            Ok("swamp test".to_owned())
        }

        async fn models(&self) -> Result<Vec<ModelSummary>> {
            self.calls.lock().unwrap().push("models".to_owned());
            Ok(vec![
                ModelSummary {
                    id: "model-id".to_owned(),
                    name: "hello-world".to_owned(),
                    model_type: "command/shell".to_owned(),
                    global_arguments_schema: Some(json!({"type": "object"})),
                    methods: vec![method()],
                },
                ModelSummary {
                    id: "second-model-id".to_owned(),
                    name: "second-world".to_owned(),
                    model_type: "command/shell".to_owned(),
                    global_arguments_schema: Some(json!({"type": "object"})),
                    methods: vec![method()],
                },
            ])
        }

        async fn model(&self, _name: &str) -> Result<ModelDetails> {
            self.calls.lock().unwrap().push("model".to_owned());
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
            self.calls.lock().unwrap().push("describe".to_owned());
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

        async fn all_data(&self) -> Result<Vec<DataArtifact>> {
            self.calls.lock().unwrap().push("all_data".to_owned());
            Ok(vec![artifact()])
        }

        async fn data(&self, _model: &str) -> Result<Vec<DataArtifact>> {
            self.calls.lock().unwrap().push("data".to_owned());
            Ok(vec![artifact()])
        }

        async fn workflows(&self) -> Result<Vec<WorkflowSummary>> {
            self.calls.lock().unwrap().push("workflows".to_owned());
            Ok(vec![WorkflowSummary {
                id: "workflow-id".to_owned(),
                name: "hello-flow".to_owned(),
                description: "Run hello".to_owned(),
                job_count: 1,
                has_inputs: false,
            }])
        }

        async fn workflow(&self, _name: &str) -> Result<WorkflowDefinition> {
            self.calls.lock().unwrap().push("workflow".to_owned());
            Ok(serde_json::from_str(include_str!("../tests/fixtures/workflow-get.json")).unwrap())
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

    fn artifact() -> DataArtifact {
        DataArtifact {
            id: "data-id".to_owned(),
            name: "result".to_owned(),
            model_id: "model-id".to_owned(),
            model_name: "hello-world".to_owned(),
            version: 2,
            content_type: "application/json".to_owned(),
            data_type: "resource".to_owned(),
            streaming: false,
            size: 10,
            created_at: "now".to_owned(),
            lifetime: "infinite".to_owned(),
            owner_type: "model-method".to_owned(),
            tags: Default::default(),
        }
    }

    fn content(version: u64) -> DataContent {
        DataContent {
            id: format!("data-{version}"),
            name: "result".to_owned(),
            model_id: "model-id".to_owned(),
            model_name: "hello-world".to_owned(),
            version,
            content_type: "application/json".to_owned(),
            data_type: "resource".to_owned(),
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
