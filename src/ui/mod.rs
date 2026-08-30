use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Tabs, Wrap},
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
    render_models(frame, app, body[0]);
    render_content(frame, app, body[1]);
    render_footer(frame, app, chunks[2]);
    render_modal(frame, app);
}

fn render_header(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let tabs = Tabs::new(vec!["1 Overview", "2 Methods", "3 Data"])
        .select(match app.tab {
            Tab::Overview => 0,
            Tab::Methods => 1,
            Tab::Data => 2,
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

fn render_content(frame: &mut Frame<'_>, app: &App, area: Rect) {
    match app.tab {
        Tab::Overview => render_overview(frame, app, area),
        Tab::Methods => render_methods(frame, app, area),
        Tab::Data => render_data(frame, app, area),
    }
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
            " Filter models ",
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
            "Arrows or j/k  Move selection\nTab              Switch model/content focus\n1/2/3            Overview/Methods/Data\nEnter            Open, load, or run\n/                Filter models\nr                Refresh\n[ / ]            Select data version\na / b            Mark comparison versions\nc                Cancel active run\nEsc              Close dialog\nq                Quit\n?                Close help",
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
}
