use std::collections::HashMap;

use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Tabs, Widget, Wrap},
};

use crate::{
    app::{App, Focus, InputMode, Tab},
    schema::FormMode,
};

const ACCENT: Color = Color::LightGreen;

pub fn render(frame: &mut Frame<'_>, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(10),
            Constraint::Length(if app.error.is_some() { 2 } else { 1 }),
        ])
        .split(frame.area());

    render_header(frame, app, chunks[0]);
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(chunks[1]);
    render_sidebar(frame, app, body[0]);
    render_content(frame, app, body[1]);
    render_footer(frame, app, chunks[2]);
    render_modal(frame, app);
}

fn render_header(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let tabs = Tabs::new(vec!["1 Overview", "2 Methods", "3 Data", "4 Workflows"])
        .select(match app.tab {
            Tab::Overview => 0,
            Tab::Methods => 1,
            Tab::Data => 2,
            Tab::Workflows => 3,
        })
        .highlight_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
        .divider(" │ ")
        .block(Block::default().title(format!(
            " Lazyswamp · {} · {} ",
            app.swamp_version,
            app.config.repo_dir.display()
        )));
    frame.render_widget(tabs, area);
}

fn render_sidebar(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if app.tab == Tab::Workflows {
        render_workflow_list(frame, app, area);
    } else {
        render_models(frame, app, area);
    }
}

fn render_models(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let items: Vec<ListItem<'_>> = app
        .visible_models()
        .map(|model| {
            ListItem::new(vec![
                Line::from(Span::styled(
                    model.name.as_str(),
                    Style::default().add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled(
                    model.model_type.as_str(),
                    Style::default().fg(Color::DarkGray),
                )),
            ])
        })
        .collect();
    let border = if app.focus == Focus::Models {
        ACCENT
    } else {
        Color::DarkGray
    };
    let title = if app.search.is_empty() {
        " Models ".to_owned()
    } else {
        format!(" Models matching “{}” ", app.search)
    };
    let list = List::new(items)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("› ");
    let mut state = ListState::default().with_selected(Some(app.model_index));
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_workflow_list(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let items: Vec<ListItem<'_>> = app
        .visible_workflows()
        .map(|workflow| {
            ListItem::new(vec![
                Line::from(Span::styled(
                    workflow.name.as_str(),
                    Style::default().add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled(
                    format!(
                        "{} job{}{}",
                        workflow.job_count,
                        if workflow.job_count == 1 { "" } else { "s" },
                        if workflow.has_inputs {
                            " · inputs"
                        } else {
                            ""
                        }
                    ),
                    Style::default().fg(Color::DarkGray),
                )),
            ])
        })
        .collect();
    let title = if app.search.is_empty() {
        " Workflows ".to_owned()
    } else {
        format!(" Workflows matching “{}” ", app.search)
    };
    let list = List::new(items)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(if app.focus == Focus::Models {
                    ACCENT
                } else {
                    Color::DarkGray
                })),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("› ");
    let mut state = ListState::default().with_selected(Some(app.workflow_index));
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_content(frame: &mut Frame<'_>, app: &App, area: Rect) {
    match app.tab {
        Tab::Overview => render_overview(frame, app, area),
        Tab::Methods => render_methods(frame, app, area),
        Tab::Data => render_data(frame, app, area),
        Tab::Workflows => render_workflows(frame, app, area),
    }
}

fn render_workflows(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ])
        .split(area);
    let summary = app.workflow.as_ref().map_or_else(
        || "Select a workflow to inspect its dependency graph.".to_owned(),
        |workflow| {
            format!(
                "{} · v{} · {} job(s) · {} step(s)\n{}",
                workflow.name,
                workflow
                    .version
                    .map_or_else(|| "?".to_owned(), |version| version.to_string()),
                workflow.jobs.len(),
                workflow.nodes().len(),
                workflow.description
            )
        },
    );
    frame.render_widget(
        Paragraph::new(summary)
            .block(content_block(" Workflow ", app))
            .wrap(Wrap { trim: false }),
        sections[0],
    );

    frame.render_widget(
        WorkflowDag::new(
            dag_nodes(app),
            app.workflow_node_index,
            content_block(" Dependency graph · j/k select node ", app),
        ),
        sections[1],
    );

    let details = app.selected_workflow_node().map_or_else(
        || "No workflow step selected.".to_owned(),
        |node| {
            let dependencies = if node.step.depends_on.is_empty() {
                "None".to_owned()
            } else {
                node.step
                    .depends_on
                    .iter()
                    .map(|dependency| {
                        let condition = dependency
                            .condition
                            .get("type")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("condition");
                        format!("{} ({condition})", dependency.step)
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let fields = node
                .step
                .task
                .fields
                .iter()
                .filter(|(name, _)| name.as_str() != "inputs")
                .map(|(name, value)| {
                    let label = match name.as_str() {
                        "modelIdOrName" => "Target",
                        "modelType" => "Type",
                        "modelName" => "Model",
                        "methodName" => "Method",
                        "workflowIdOrName" => "Workflow",
                        "globalArgs" => "Global arguments",
                        "prompt" => "Prompt",
                        "timeout" => "Timeout",
                        "expr" => "Expression",
                        "message" => "Message",
                        "severity" => "Severity",
                        _ => name,
                    };
                    let value = value
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| serde_json::to_string(value).unwrap_or_default());
                    format!("{label}: {value}")
                })
                .collect::<Vec<_>>()
                .join(" · ");
            let inputs = node
                .step
                .task
                .fields
                .get("inputs")
                .map(|inputs| serde_json::to_string(inputs).unwrap_or_default())
                .unwrap_or_else(|| "None".to_owned());
            format!(
                "Job: {} · Step: {}\nTask: {} · Layer: {}\n{}\nDepends on: {}\nInputs: {}\n{}",
                node.job.name,
                node.step.name,
                node.step.task.task_type,
                node.layer,
                fields,
                dependencies,
                inputs,
                node.step.description
            )
        },
    );
    frame.render_widget(
        Paragraph::new(details)
            .block(
                Block::default()
                    .title(" Selected step ")
                    .borders(Borders::ALL),
            )
            .wrap(Wrap { trim: false }),
        sections[2],
    );
}

#[derive(Debug)]
struct DagNode {
    name: String,
    layer: usize,
    dependencies: Vec<usize>,
}

fn dag_nodes(app: &App) -> Vec<DagNode> {
    let workflow_nodes = app.workflow_nodes();
    let indices: HashMap<(&str, &str), usize> = workflow_nodes
        .iter()
        .enumerate()
        .map(|(index, node)| ((node.job.name.as_str(), node.step.name.as_str()), index))
        .collect();

    workflow_nodes
        .iter()
        .map(|node| {
            let mut dependencies: Vec<usize> = node
                .step
                .depends_on
                .iter()
                .filter_map(|dependency| {
                    indices
                        .get(&(node.job.name.as_str(), dependency.step.as_str()))
                        .copied()
                })
                .collect();
            if node.step.depends_on.is_empty() {
                for job_dependency in &node.job.depends_on {
                    dependencies.extend(
                        workflow_nodes
                            .iter()
                            .enumerate()
                            .filter(|(_, candidate)| candidate.job.name == job_dependency.job)
                            .filter(|(_, candidate)| {
                                !workflow_nodes.iter().any(|other| {
                                    other.job.name == candidate.job.name
                                        && other.step.depends_on.iter().any(|dependency| {
                                            dependency.step == candidate.step.name
                                        })
                                })
                            })
                            .map(|(index, _)| index),
                    );
                }
            }
            dependencies.sort_unstable();
            dependencies.dedup();
            DagNode {
                name: node.step.name.clone(),
                layer: node.layer,
                dependencies,
            }
        })
        .collect()
}

struct WorkflowDag<'a> {
    nodes: Vec<DagNode>,
    selected: usize,
    block: Block<'a>,
}

impl<'a> WorkflowDag<'a> {
    fn new(nodes: Vec<DagNode>, selected: usize, block: Block<'a>) -> Self {
        Self {
            nodes,
            selected,
            block,
        }
    }
}

impl Widget for WorkflowDag<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let inner = self.block.inner(area);
        self.block.render(area, buffer);
        if self.nodes.is_empty() || inner.width < 5 || inner.height == 0 {
            buffer.set_string(
                inner.x,
                inner.y,
                "No steps",
                Style::default().fg(Color::DarkGray),
            );
            return;
        }

        let width = usize::from(inner.width);
        let height = usize::from(inner.height);
        let layer_count = self.nodes.iter().map(|node| node.layer).max().unwrap_or(0) + 1;
        let visible_count = layer_count.min(((width + 3) / 11).max(1));
        let selected_layer = self.nodes.get(self.selected).map_or(0, |node| node.layer);
        let first_layer = selected_layer
            .saturating_sub(visible_count / 2)
            .min(layer_count.saturating_sub(visible_count));
        let last_layer = first_layer + visible_count;
        let gap_total = 3 * visible_count.saturating_sub(1);
        let node_width = width
            .saturating_sub(gap_total)
            .checked_div(visible_count)
            .unwrap_or(width)
            .clamp(5, 18);
        let x_span = width.saturating_sub(node_width);

        let mut positions = vec![None; self.nodes.len()];
        for layer in first_layer..last_layer {
            let layer_nodes: Vec<usize> = self
                .nodes
                .iter()
                .enumerate()
                .filter(|(_, node)| node.layer == layer)
                .map(|(index, _)| index)
                .collect();
            let local_layer = layer - first_layer;
            let x = if visible_count == 1 {
                x_span / 2
            } else {
                local_layer * x_span / (visible_count - 1)
            };
            for (row, index) in layer_nodes.iter().enumerate() {
                let y = if layer_nodes.len() == 1 {
                    height / 2
                } else {
                    row * height.saturating_sub(1) / (layer_nodes.len() - 1)
                };
                positions[*index] = Some((x, y));
            }
        }

        let mut grid = vec![vec![' '; width]; height];
        for (target_index, target) in self.nodes.iter().enumerate() {
            let Some((target_x, target_y)) = positions[target_index] else {
                continue;
            };
            for dependency in &target.dependencies {
                let Some((source_x, source_y)) = positions.get(*dependency).copied().flatten()
                else {
                    if self.nodes[*dependency].layer < first_layer && target_x > 0 {
                        draw_horizontal(&mut grid, 0, target_x, target_y);
                        put_route(&mut grid, target_x - 1, target_y, '▶');
                    }
                    continue;
                };
                route_edge(
                    &mut grid,
                    source_x + node_width,
                    source_y,
                    target_x,
                    target_y,
                );
            }
        }

        let route_style = Style::default().fg(Color::DarkGray);
        for (y, row) in grid.iter().enumerate() {
            for (x, character) in row.iter().enumerate() {
                if *character != ' ' {
                    buffer[(inner.x + x as u16, inner.y + y as u16)]
                        .set_char(*character)
                        .set_style(route_style);
                }
            }
        }

        for (index, node) in self.nodes.iter().enumerate() {
            let Some((x, y)) = positions[index] else {
                continue;
            };
            let label_width = node_width.saturating_sub(2);
            let mut name: String = node.name.chars().take(label_width).collect();
            if node.name.chars().count() > label_width && !name.is_empty() {
                name.pop();
                name.push('…');
            }
            let label = format!("[{name:<label_width$}]");
            let style = if index == self.selected {
                Style::default()
                    .fg(ACCENT)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else {
                Style::default().fg(Color::White)
            };
            buffer.set_string(inner.x + x as u16, inner.y + y as u16, label, style);
        }

        if first_layer > 0 {
            buffer.set_string(inner.x, inner.y, "…", Style::default().fg(Color::DarkGray));
        }
        if last_layer < layer_count {
            buffer.set_string(
                inner.x + inner.width.saturating_sub(1),
                inner.y,
                "…",
                Style::default().fg(Color::DarkGray),
            );
        }
    }
}

fn route_edge(
    grid: &mut [Vec<char>],
    source_x: usize,
    source_y: usize,
    target_x: usize,
    target_y: usize,
) {
    if target_x <= source_x {
        return;
    }
    if source_y == target_y {
        draw_horizontal(grid, source_x, target_x, source_y);
        put_route(grid, target_x - 1, target_y, '▶');
        return;
    }

    let middle = source_x + (target_x - source_x) / 2;
    draw_horizontal(grid, source_x, middle + 1, source_y);
    draw_horizontal(grid, middle, target_x, target_y);
    let (top, bottom) = if source_y < target_y {
        (source_y, target_y)
    } else {
        (target_y, source_y)
    };
    for y in top + 1..bottom {
        put_route(grid, middle, y, '│');
    }
    let (source_corner, target_corner) = if source_y < target_y {
        ('┐', '└')
    } else {
        ('┘', '┌')
    };
    put_route(grid, middle, source_y, source_corner);
    put_route(grid, middle, target_y, target_corner);
    put_route(grid, target_x - 1, target_y, '▶');
}

fn draw_horizontal(grid: &mut [Vec<char>], start: usize, end: usize, y: usize) {
    for x in start..end {
        put_route(grid, x, y, '─');
    }
}

fn put_route(grid: &mut [Vec<char>], x: usize, y: usize, character: char) {
    let Some(cell) = grid.get_mut(y).and_then(|row| row.get_mut(x)) else {
        return;
    };
    *cell = match (*cell, character) {
        (' ', character) => character,
        ('▶', _) | (_, '▶') => '▶',
        (existing, new) if existing == new => existing,
        _ => '┼',
    };
}

fn render_overview(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let text = if let Some(detail) = &app.detail {
        let tags = serde_json::to_string_pretty(&detail.tags).unwrap_or_default();
        let arguments = serde_json::to_string_pretty(&detail.global_arguments).unwrap_or_default();
        format!(
            "Name:         {}\nID:           {}\nType:         {}\nType version: {}\nDefinition:   v{}\n\nTags\n{}\n\nGlobal arguments\n{}",
            detail.name,
            detail.id,
            detail.model_type,
            detail.type_version.as_deref().unwrap_or("unknown"),
            detail
                .version
                .map_or_else(|| "?".to_owned(), |v| v.to_string()),
            tags,
            arguments
        )
    } else {
        "Select a model to inspect it.".to_owned()
    };
    frame.render_widget(
        panel(
            " Overview ",
            app,
            Paragraph::new(text).wrap(Wrap { trim: false }),
        ),
        area,
    );
}

fn render_methods(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);
    let methods = app
        .type_description
        .as_ref()
        .map(|description| description.methods.as_slice())
        .unwrap_or_default();
    let items: Vec<ListItem<'_>> = methods
        .iter()
        .map(|method| ListItem::new(method.name.as_str()))
        .collect();
    let list = List::new(items)
        .block(content_block(" Methods ", app))
        .highlight_style(Style::default().bg(Color::DarkGray).fg(Color::White))
        .highlight_symbol("› ");
    let mut state = ListState::default().with_selected(Some(app.method_index));
    frame.render_stateful_widget(list, chunks[0], &mut state);

    let detail = if !app.run_logs.is_empty() {
        app.run_logs.join("\n")
    } else if let Some(method) = app.selected_method() {
        let outputs = app
            .type_description
            .as_ref()
            .and_then(|description| {
                (!description.data_output_specs.is_empty()).then(|| {
                    serde_json::to_string_pretty(&description.data_output_specs).unwrap_or_default()
                })
            })
            .unwrap_or_else(|| "None declared".to_owned());
        format!(
            "{}\n\nArguments\n{}\n\nOutput specifications\n{}",
            method.description,
            serde_json::to_string_pretty(&method.arguments).unwrap_or_default(),
            outputs
        )
    } else {
        "This model exposes no methods.".to_owned()
    };
    let title = if app.run_receiver.is_some() {
        " Run log (c to cancel) "
    } else {
        " Method details (Enter to run) "
    };
    frame.render_widget(
        Paragraph::new(detail)
            .block(Block::default().title(title).borders(Borders::ALL))
            .wrap(Wrap { trim: false }),
        chunks[1],
    );
}

fn render_data(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
        .split(area);
    let items: Vec<ListItem<'_>> = app
        .artifacts
        .iter()
        .map(|artifact| {
            ListItem::new(vec![
                Line::from(format!("{}  v{}", artifact.name, artifact.version)),
                Line::from(Span::styled(
                    format!(
                        "{} · {} · {} B",
                        artifact.data_type, artifact.content_type, artifact.size
                    ),
                    Style::default().fg(Color::DarkGray),
                )),
            ])
        })
        .collect();
    let list = List::new(items)
        .block(content_block(" Data ", app))
        .highlight_style(Style::default().bg(Color::DarkGray).fg(Color::White))
        .highlight_symbol("› ");
    let mut state = ListState::default().with_selected(Some(app.artifact_index));
    frame.render_stateful_widget(list, chunks[0], &mut state);

    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(9), Constraint::Min(5)])
        .split(chunks[1]);
    let metadata = app.selected_artifact().map_or_else(
        || "No data produced by this model.".to_owned(),
        |artifact| {
            let mut metadata = format!(
                "Name: {}\nVersion: {}\nKind: {}\nContent: {}\nSize: {} B · Created: {}",
                artifact.name,
                artifact.version,
                artifact.data_type,
                artifact.content_type,
                artifact.size,
                artifact.created_at
            );
            if let Some(content) = app
                .content
                .as_ref()
                .filter(|content| content.name == artifact.name)
            {
                metadata.push_str(&format!(
                    "\nLifetime: {} · Owner: {} · Streaming: {}\nTags: {}",
                    if content.lifetime.is_empty() {
                        "unknown"
                    } else {
                        &content.lifetime
                    },
                    if content.effective_owner_type().is_empty() {
                        "unknown"
                    } else {
                        content.effective_owner_type()
                    },
                    content.streaming,
                    serde_json::to_string(&content.tags).unwrap_or_default()
                ));
            }
            metadata
        },
    );
    frame.render_widget(
        Paragraph::new(metadata).block(Block::default().title(" Metadata ").borders(Borders::ALL)),
        right[0],
    );

    let mut body = if let Some(diff) = &app.diff {
        diff.clone()
    } else {
        app.content_text().unwrap_or_else(|| {
            if app.content.is_some() {
                "Binary content is not previewed.".to_owned()
            } else {
                "Enter loads content and version history.".to_owned()
            }
        })
    };
    if !app.versions.is_empty() {
        let versions = app
            .versions
            .iter()
            .enumerate()
            .map(|(index, version)| {
                let cursor = if index == app.version_cursor {
                    "›"
                } else {
                    " "
                };
                let a = if app.compare_a == Some(index) {
                    "A"
                } else {
                    " "
                };
                let b = if app.compare_b == Some(index) {
                    "B"
                } else {
                    " "
                };
                format!("{cursor}{a}{b} v{}", version.version)
            })
            .collect::<Vec<_>>()
            .join("  ");
        body = format!("Versions: {versions}\n[ ] select · a/b compare · Enter load\n\n{body}");
    }
    frame.render_widget(
        Paragraph::new(body)
            .block(
                Block::default()
                    .title(" Content / diff ")
                    .borders(Borders::ALL),
            )
            .wrap(Wrap { trim: false }),
        right[1],
    );
}

fn render_footer(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let mut lines = vec![Line::from(vec![
        Span::styled(app.status.as_str(), Style::default().fg(ACCENT)),
        Span::raw("  ·  / filter · Tab focus · r refresh · ? help · q quit"),
    ])];
    if let Some(error) = &app.error {
        lines.push(Line::from(Span::styled(
            error.as_str(),
            Style::default().fg(Color::LightRed),
        )));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

fn render_modal(frame: &mut Frame<'_>, app: &App) {
    match &app.mode {
        InputMode::Normal => {}
        InputMode::Search => render_popup(
            frame,
            60,
            5,
            if app.tab == Tab::Workflows {
                " Filter workflows "
            } else {
                " Filter models "
            },
            format!("{}█\nEnter applies · Esc clears", app.search),
        ),
        InputMode::MethodForm => render_form_popup(frame, app),
        InputMode::Review => {
            let model = app
                .selected_model()
                .map_or("?", |model| model.name.as_str());
            let method = app
                .form
                .as_ref()
                .map_or("?", |form| form.method.name.as_str());
            let payload = app
                .redacted_pending()
                .and_then(|value| serde_json::to_string_pretty(&value).ok())
                .unwrap_or_default();
            render_popup(
                frame,
                76,
                70,
                " Review method invocation ",
                format!(
                    "Target: {model}\nMethod: {method}\n\n{payload}\n\ny/Enter run · n/Esc edit"
                ),
            );
        }
        InputMode::DestructiveConfirm(text) => {
            let model = app
                .selected_model()
                .map_or("?", |model| model.name.as_str());
            render_popup(
                frame,
                70,
                9,
                " Destructive-looking method ",
                format!(
                    "This method may be destructive.\nType the model name `{model}` to continue:\n\n{text}█\n\nEnter confirms · Esc returns"
                ),
            );
        }
        InputMode::LargeConfirm(_) => render_popup(
            frame,
            62,
            8,
            " Large content ",
            format!(
                "This operation exceeds the {} byte preview threshold.\n\ny/Enter load anyway · n/Esc cancel",
                app.config.preview_limit
            ),
        ),
        InputMode::Help => render_popup(
            frame,
            70,
            70,
            " Help ",
            "Arrows or j/k  Move selection\nTab              Switch list/content focus\n1/2/3/4          Overview/Methods/Data/Workflows\nEnter            Open, load, or run\n/                Filter current list\nr                Refresh\n[ / ]            Select data version\na / b            Mark comparison versions\nc                Cancel active run\nEsc              Close dialog\nq                Quit\n?                Close help",
        ),
    }
}

fn render_form_popup(frame: &mut Frame<'_>, app: &App) {
    let Some(form) = &app.form else {
        return;
    };
    let text = match &form.mode {
        FormMode::RawJson(value) => format!(
            "The schema uses constructs not represented by field widgets.\nEdit the complete JSON value:\n\n{value}█\n\nEnter validates · Esc cancels"
        ),
        FormMode::Fields(fields) => {
            let mut lines = vec!["Edit inputs; optional empty values are omitted.".to_owned()];
            for (index, field) in fields.iter().enumerate() {
                let marker = if index == form.selected_field {
                    "›"
                } else {
                    " "
                };
                let required = if field.required { "*" } else { " " };
                lines.push(format!(
                    "{marker} {required}{}: {}",
                    field.label,
                    field.display_value()
                ));
                if index == form.selected_field && !field.description.is_empty() {
                    lines.push(format!("    {}", field.description));
                }
            }
            lines.push(String::new());
            lines.push("Enter validates · arrows select · Space cycles · Esc cancels".to_owned());
            lines.join("\n")
        }
    };
    render_popup(frame, 80, 80, " Method inputs ", text);
}

fn render_popup(
    frame: &mut Frame<'_>,
    width: u16,
    height: u16,
    title: &str,
    text: impl Into<Text<'static>>,
) {
    let area = centered_rect(width, height, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(text)
            .block(
                Block::default()
                    .title(title)
                    .title_alignment(Alignment::Center)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(ACCENT)),
            )
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let width = if width <= 100 {
        area.width.saturating_mul(width) / 100
    } else {
        width.min(area.width)
    };
    let height = if height <= 100 {
        area.height.saturating_mul(height) / 100
    } else {
        height.min(area.height)
    };
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

fn content_block<'a>(title: &'a str, app: &App) -> Block<'a> {
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if app.focus == Focus::Content {
            ACCENT
        } else {
            Color::DarkGray
        }))
}

fn panel<'a, W>(title: &'a str, app: &App, widget: W) -> W
where
    W: PanelWidget<'a>,
{
    widget.with_panel(content_block(title, app))
}

trait PanelWidget<'a> {
    fn with_panel(self, block: Block<'a>) -> Self;
}

impl<'a> PanelWidget<'a> for Paragraph<'a> {
    fn with_panel(self, block: Block<'a>) -> Self {
        self.block(block)
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc};

    use ratatui::{Terminal, backend::TestBackend};

    use crate::{
        app::App,
        config::{Config, DEFAULT_PREVIEW_LIMIT},
        swamp::SwampCli,
    };

    #[test]
    fn renders_empty_application_shell() {
        let config = Config {
            repo_dir: PathBuf::from("/repo"),
            swamp_bin: PathBuf::from("swamp"),
            preview_limit: DEFAULT_PREVIEW_LIMIT,
        };
        let client = Arc::new(SwampCli::new(
            config.swamp_bin.clone(),
            config.repo_dir.clone(),
        ));
        let app = App::new(config, client);
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| super::render(frame, &app)).unwrap();
        let rendered: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(rendered.contains("Lazyswamp"));
        assert!(rendered.contains("Select a model"));
    }

    #[test]
    fn renders_interactive_workflow_graph() {
        let config = Config {
            repo_dir: PathBuf::from("/repo"),
            swamp_bin: PathBuf::from("swamp"),
            preview_limit: DEFAULT_PREVIEW_LIMIT,
        };
        let client = Arc::new(SwampCli::new(
            config.swamp_bin.clone(),
            config.repo_dir.clone(),
        ));
        let mut app = App::new(config, client);
        app.tab = crate::app::Tab::Workflows;
        app.workflow = Some(
            serde_json::from_str(include_str!("../../tests/fixtures/workflow-get.json")).unwrap(),
        );
        app.workflows = vec![crate::swamp::WorkflowSummary {
            id: "workflow-id".to_owned(),
            name: "update-weight-dashboard".to_owned(),
            description: "Update the dashboard".to_owned(),
            job_count: 1,
            has_inputs: false,
        }];
        app.filtered_workflows = vec![0];
        app.workflow_node_index = 3;

        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| super::render(frame, &app)).unwrap();
        let rendered: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(rendered.contains("Dependency graph"));
        assert!(rendered.contains("render-dashboard"));
        assert!(rendered.contains("collect-dieter"));
        assert!(rendered.contains("Selected step"));
        assert!(rendered.contains('▶'));
        assert!(rendered.contains('─'));
        assert!(rendered.contains('│') || rendered.contains('┼'));
    }
}
