use crate::{
    components::{COLOR_COMPLETED, COLOR_HIERARCHY, COLOR_INTERACTIVE},
    config::{
        Action, KeyContext, Keymap, StatusComponentConfig, StatusComponentKind, StatusOverflow,
        StatuslineLayout, TextModifier, ThemeConfig, ThemePreset, TuiRunContext, builtin_kind,
    },
    model::{Activity, ActivitySummary, ActivityVariant, NixActivityState, TaskDisplayStatus},
};
use crossterm::style::Color;
use devenv_activity::ProcessStatus;
use std::collections::BTreeMap;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatuslineMode {
    Main,
    Logs,
    Search,
    Prompt,
}

#[derive(Clone, Debug, Default)]
pub struct StatuslineData {
    pub summary: ActivitySummary,
    pub selected: Option<Activity>,
    pub context: TuiRunContext,
    pub hidden_processes: usize,
    pub log_mode: Option<String>,
    pub log_current: Option<usize>,
    pub log_total: Option<usize>,
    pub retained_logs: Option<usize>,
    pub discarded_logs: Option<usize>,
    pub search_query: Option<String>,
    pub search_current: Option<usize>,
    pub search_total: Option<usize>,
    pub search_result: Option<String>,
    pub prompt: Option<String>,
    pub pending_key: Option<String>,
    pub key_hints: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SegmentStyle {
    pub foreground: Option<Color>,
    pub background: Option<Color>,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub reverse: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedSegment {
    pub name: String,
    pub content: String,
    pub style: SegmentStyle,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RenderedStatusline {
    pub left: Vec<RenderedSegment>,
    pub center: Vec<RenderedSegment>,
    pub right: Vec<RenderedSegment>,
    pub separator: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Zone {
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug)]
struct Candidate {
    zone: Zone,
    name: String,
    kind: StatusComponentKind,
    preserves_interaction_value: bool,
    compact: String,
    content: String,
    priority: u8,
    required: bool,
    overflow: StatusOverflow,
    style: SegmentStyle,
}

pub fn render_statusline(
    mode: StatuslineMode,
    width: u16,
    preferences: &crate::config::TuiPreferences,
    data: &StatuslineData,
) -> RenderedStatusline {
    if width == 0
        || !preferences.statusline.enabled
            && matches!(mode, StatuslineMode::Main | StatuslineMode::Logs)
    {
        return RenderedStatusline::default();
    }
    let layout = match mode {
        StatuslineMode::Main => &preferences.statusline.layouts.main,
        StatuslineMode::Logs => &preferences.statusline.layouts.logs,
        StatuslineMode::Search => &preferences.statusline.layouts.search,
        StatuslineMode::Prompt => &preferences.statusline.layouts.prompt,
    };
    let mut candidates = build_candidates(
        layout,
        &preferences.statusline.components,
        &preferences.theme,
        data,
    );
    ensure_interaction_candidates(mode, &mut candidates, &preferences.theme, data);
    let separator = preferences.statusline.separator.clone();
    fit(&mut candidates, width as usize, &separator);
    let mut rendered = RenderedStatusline {
        separator,
        ..RenderedStatusline::default()
    };
    for candidate in candidates {
        if candidate.content.is_empty() {
            continue;
        }
        let segment = RenderedSegment {
            name: candidate.name,
            content: candidate.content,
            style: candidate.style,
        };
        match candidate.zone {
            Zone::Left => rendered.left.push(segment),
            Zone::Center => rendered.center.push(segment),
            Zone::Right => rendered.right.push(segment),
        }
    }
    rendered
}

pub(crate) fn action_key_hints(
    keymap: &Keymap,
    context: KeyContext,
    actions: impl IntoIterator<Item = Action>,
    width: u16,
) -> Option<String> {
    let actions = actions.into_iter().collect::<Vec<_>>();
    let compact = width < 120 || actions.len() > 8 && width < 240;
    let hints = actions
        .into_iter()
        .filter_map(|action| {
            if compact {
                keymap.key_label(context, action, true)
            } else {
                keymap.hint(context, action)
            }
        })
        .collect::<Vec<_>>();
    (!hints.is_empty()).then(|| hints.join(if compact { " " } else { " • " }))
}

pub(crate) fn interrupt_prompt_key_hints(keymap: &Keymap, attached: bool, width: u16) -> String {
    let actions = if attached {
        [Action::Cancel, Action::StopManager]
    } else {
        [Action::Cancel, Action::Quit]
    };
    let compact = width < 120;
    let mut hints = actions
        .into_iter()
        .filter_map(|action| {
            if compact {
                keymap
                    .key_label(KeyContext::Prompt, action, true)
                    .map(|key| {
                        let label = match action {
                            Action::StopManager => "stop",
                            _ => action.label(),
                        };
                        format!("{key}:{label}")
                    })
            } else {
                keymap.hint(KeyContext::Prompt, action)
            }
        })
        .collect::<Vec<_>>();
    hints.push(if attached && compact {
        "^C:detach".to_string()
    } else if attached {
        "Ctrl-C detach".to_string()
    } else if compact {
        "^C:quit".to_string()
    } else {
        "Ctrl-C quit".to_string()
    });
    hints.join(if compact { " " } else { " • " })
}

fn build_candidates(
    layout: &StatuslineLayout,
    overrides: &BTreeMap<String, StatusComponentConfig>,
    theme: &ThemeConfig,
    data: &StatuslineData,
) -> Vec<Candidate> {
    let mut candidates = Vec::new();
    for (zone, names) in [
        (Zone::Left, &layout.left),
        (Zone::Center, &layout.center),
        (Zone::Right, &layout.right),
    ] {
        for name in names {
            let config = overrides.get(name).cloned().unwrap_or_default();
            let Some(kind) = config.kind.or_else(|| builtin_kind(name)) else {
                continue;
            };
            let values = component_values(kind, &config, data);
            let default = default_format(kind);
            let full_format = config.format.as_deref().unwrap_or(default.0);
            let compact_format = config.compact_format.as_deref().unwrap_or(default.1);
            let full = apply_format(full_format, &values);
            let compact = apply_format(compact_format, &values);
            if !config.show_empty && component_is_empty(kind, data) {
                continue;
            }
            let full = limit_width(full, config.max_width.map(usize::from));
            let compact = limit_width(compact, config.max_width.map(usize::from));
            candidates.push(Candidate {
                zone,
                name: name.clone(),
                kind,
                preserves_interaction_value: interaction_variable(kind).is_some_and(|variable| {
                    format_uses_variable(full_format, variable)
                        && format_uses_variable(compact_format, variable)
                }),
                content: full.clone(),
                compact,
                priority: config.priority,
                required: config.required
                    || matches!(
                        kind,
                        StatusComponentKind::Summary | StatusComponentKind::Prompt
                    ),
                overflow: config.overflow,
                style: resolve_style(theme, name, &config),
            });
        }
    }
    candidates
}

fn ensure_interaction_candidates(
    mode: StatuslineMode,
    candidates: &mut Vec<Candidate>,
    theme: &ThemeConfig,
    data: &StatuslineData,
) {
    let required = match mode {
        StatuslineMode::Prompt => [StatusComponentKind::Prompt, StatusComponentKind::KeyHints],
        StatuslineMode::Search => [StatusComponentKind::Search, StatusComponentKind::KeyHints],
        StatuslineMode::Main | StatuslineMode::Logs => return,
    };
    for kind in required {
        if component_is_empty(kind, data)
            || candidates
                .iter()
                .any(|candidate| candidate_preserves_value(candidate, kind, data))
        {
            continue;
        }
        let name = match kind {
            StatusComponentKind::Prompt => "prompt",
            StatusComponentKind::Search => "search",
            StatusComponentKind::KeyHints => "key_hints",
            _ => unreachable!(),
        };
        let config = StatusComponentConfig::default();
        let values = component_values(kind, &config, data);
        let format = default_format(kind);
        let content = apply_format(format.0, &values);
        candidates.push(Candidate {
            zone: if kind == StatusComponentKind::KeyHints {
                Zone::Right
            } else {
                Zone::Left
            },
            name: name.to_string(),
            kind,
            preserves_interaction_value: true,
            compact: apply_format(format.1, &values),
            content,
            priority: u8::MAX,
            required: true,
            overflow: StatusOverflow::Truncate,
            style: resolve_style(theme, name, &config),
        });
    }
}

fn candidate_preserves_value(
    candidate: &Candidate,
    kind: StatusComponentKind,
    data: &StatuslineData,
) -> bool {
    if candidate.kind != kind {
        return false;
    }
    if !candidate.preserves_interaction_value {
        return false;
    }
    let value = match kind {
        StatusComponentKind::Prompt => data.prompt.as_deref(),
        StatusComponentKind::Search => data.search_query.as_deref(),
        StatusComponentKind::KeyHints => data.key_hints.as_deref(),
        _ => None,
    };
    value.is_some_and(|value| {
        (kind == StatusComponentKind::Search
            && value.is_empty()
            && !candidate.content.is_empty()
            && !candidate.compact.is_empty())
            || (candidate.content.contains(value) && candidate.compact.contains(value))
    })
}

fn interaction_variable(kind: StatusComponentKind) -> Option<&'static str> {
    match kind {
        StatusComponentKind::Prompt => Some("prompt"),
        StatusComponentKind::Search => Some("query"),
        StatusComponentKind::KeyHints => Some("hints"),
        _ => None,
    }
}

fn format_uses_variable(mut format: &str, variable: &str) -> bool {
    while let Some(start) = format.find('{') {
        format = &format[start + 1..];
        if let Some(escaped) = format.strip_prefix('{') {
            format = escaped;
            continue;
        }
        let Some(end) = format.find('}') else {
            return false;
        };
        if &format[..end] == variable {
            return true;
        }
        format = &format[end + 1..];
    }
    false
}

fn component_is_empty(kind: StatusComponentKind, data: &StatuslineData) -> bool {
    let summary = &data.summary;
    match kind {
        StatusComponentKind::Summary => summary_text(summary).is_empty(),
        StatusComponentKind::Builds => {
            summary.active_builds + summary.completed_builds + summary.failed_builds == 0
        }
        StatusComponentKind::Downloads => {
            summary.active_downloads + summary.completed_downloads == 0
        }
        StatusComponentKind::Queries => summary.active_queries + summary.completed_queries == 0,
        StatusComponentKind::Tasks => {
            summary.running_tasks + summary.completed_tasks + summary.failed_tasks == 0
        }
        StatusComponentKind::Processes => summary.total_processes == 0,
        StatusComponentKind::Profiles => data.context.profiles.is_empty(),
        StatusComponentKind::Project => data.context.project_root.is_none(),
        StatusComponentKind::Command => data.context.command.as_deref().is_none_or(str::is_empty),
        StatusComponentKind::Shell => data.context.shell.as_deref().is_none_or(str::is_empty),
        StatusComponentKind::Elapsed => data.context.started_at.is_none(),
        StatusComponentKind::Selected => data.selected.is_none(),
        StatusComponentKind::LogMode => data.log_mode.as_deref().is_none_or(str::is_empty),
        StatusComponentKind::LogPosition => data.log_total.unwrap_or_default() == 0,
        StatusComponentKind::RetainedLogs => {
            data.retained_logs.unwrap_or_default() + data.discarded_logs.unwrap_or_default() == 0
        }
        StatusComponentKind::Search => data.search_query.is_none(),
        StatusComponentKind::Prompt => data.prompt.as_deref().is_none_or(str::is_empty),
        StatusComponentKind::PendingKey => data.pending_key.as_deref().is_none_or(str::is_empty),
        StatusComponentKind::KeyHints => data.key_hints.as_deref().is_none_or(str::is_empty),
        StatusComponentKind::Text => false,
    }
}

fn component_values(
    kind: StatusComponentKind,
    config: &StatusComponentConfig,
    data: &StatuslineData,
) -> BTreeMap<&'static str, String> {
    let summary = &data.summary;
    let mut values = BTreeMap::new();
    match kind {
        StatusComponentKind::Summary => {
            values.insert("summary", summary_text(summary));
        }
        StatusComponentKind::Builds => {
            let observed = summary.active_builds + summary.completed_builds + summary.failed_builds;
            metric_values(
                &mut values,
                summary.active_builds,
                summary.completed_builds,
                summary.failed_builds,
                observed,
                summary.expected_builds.map(|value| value as usize),
            );
        }
        StatusComponentKind::Downloads => {
            let total = summary.active_downloads + summary.completed_downloads;
            metric_values(
                &mut values,
                summary.active_downloads,
                summary.completed_downloads,
                0,
                total,
                summary.expected_downloads.map(|value| value as usize),
            );
        }
        StatusComponentKind::Queries => metric_values(
            &mut values,
            summary.active_queries,
            summary.completed_queries,
            0,
            summary.active_queries + summary.completed_queries,
            None,
        ),
        StatusComponentKind::Tasks => metric_values(
            &mut values,
            summary.running_tasks,
            summary.completed_tasks,
            summary.failed_tasks,
            summary.running_tasks + summary.completed_tasks + summary.failed_tasks,
            None,
        ),
        StatusComponentKind::Processes => {
            values.insert("running", summary.running_processes.to_string());
            values.insert("stopped", summary.stopped_processes.to_string());
            values.insert("failed", summary.failed_processes.to_string());
            values.insert("hidden", data.hidden_processes.to_string());
            values.insert("total", summary.total_processes.to_string());
        }
        StatusComponentKind::Profiles => {
            values.insert("profiles", data.context.profiles.join(","));
            values.insert("count", data.context.profiles.len().to_string());
        }
        StatusComponentKind::Project => {
            values.insert("name", data.context.project_name().unwrap_or_default());
            values.insert(
                "path",
                data.context
                    .project_root
                    .as_deref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default(),
            );
        }
        StatusComponentKind::Command => {
            values.insert("command", data.context.command.clone().unwrap_or_default());
        }
        StatusComponentKind::Shell => {
            values.insert("shell", data.context.shell.clone().unwrap_or_default());
        }
        StatusComponentKind::Elapsed => {
            values.insert(
                "elapsed",
                data.context
                    .started_at
                    .map(format_elapsed)
                    .unwrap_or_default(),
            );
        }
        StatusComponentKind::Selected => {
            values.insert(
                "name",
                data.selected
                    .as_ref()
                    .map(|activity| activity.short_name.clone())
                    .unwrap_or_default(),
            );
            values.insert(
                "status",
                data.selected
                    .as_ref()
                    .map(|activity| selected_status(activity).to_string())
                    .unwrap_or_default(),
            );
        }
        StatusComponentKind::LogMode => {
            values.insert("mode", data.log_mode.clone().unwrap_or_default());
        }
        StatusComponentKind::LogPosition => {
            let current = data.log_current.unwrap_or_default();
            let total = data.log_total.unwrap_or_default();
            values.insert("current", current.to_string());
            values.insert("total", total.to_string());
            values.insert(
                "percent",
                current
                    .saturating_mul(100)
                    .checked_div(total)
                    .map_or_else(String::new, |percent| percent.to_string()),
            );
        }
        StatusComponentKind::RetainedLogs => {
            let retained = data.retained_logs.unwrap_or_default();
            let discarded = data.discarded_logs.unwrap_or_default();
            values.insert("retained", retained.to_string());
            values.insert("discarded", discarded.to_string());
            values.insert("total", (retained + discarded).to_string());
        }
        StatusComponentKind::Search => {
            values.insert("query", data.search_query.clone().unwrap_or_default());
            values.insert(
                "current",
                data.search_current.unwrap_or_default().to_string(),
            );
            values.insert("total", data.search_total.unwrap_or_default().to_string());
            values.insert("result", data.search_result.clone().unwrap_or_default());
        }
        StatusComponentKind::Prompt => {
            values.insert("prompt", data.prompt.clone().unwrap_or_default());
        }
        StatusComponentKind::PendingKey => {
            values.insert("keys", data.pending_key.clone().unwrap_or_default());
        }
        StatusComponentKind::KeyHints => {
            values.insert("hints", data.key_hints.clone().unwrap_or_default());
        }
        StatusComponentKind::Text => {
            values.insert("text", config.text.clone().unwrap_or_default());
        }
    }
    values
}

fn selected_status(activity: &Activity) -> &'static str {
    match &activity.variant {
        ActivityVariant::Task(task) => match task.status {
            TaskDisplayStatus::Pending => "pending",
            TaskDisplayStatus::Running => "running",
            TaskDisplayStatus::Success => "success",
            TaskDisplayStatus::Failed => "failed",
            TaskDisplayStatus::Skipped => "skipped",
            TaskDisplayStatus::Cancelled => "cancelled",
        },
        ActivityVariant::Process(process) => match process.status {
            ProcessStatus::NotStarted => "not started",
            ProcessStatus::Waiting => "waiting",
            ProcessStatus::Starting => "starting",
            ProcessStatus::Running => "running",
            ProcessStatus::Ready => "ready",
            ProcessStatus::Restarting => "restarting",
            ProcessStatus::Stopping => "stopping",
            ProcessStatus::Stopped => "stopped",
            ProcessStatus::Exited => "exited",
            ProcessStatus::GaveUp => "gave up",
        },
        _ => match activity.state {
            NixActivityState::Queued => "queued",
            NixActivityState::Active => "active",
            NixActivityState::Completed { success: true, .. } => "completed",
            NixActivityState::Completed { success: false, .. } => "failed",
        },
    }
}

fn metric_values(
    values: &mut BTreeMap<&'static str, String>,
    active: usize,
    completed: usize,
    failed: usize,
    total: usize,
    expected: Option<usize>,
) {
    values.insert("active", active.to_string());
    values.insert("completed", completed.to_string());
    values.insert("failed", failed.to_string());
    values.insert("total", total.to_string());
    values.insert("expected", expected.unwrap_or(total).max(total).to_string());
}

fn default_format(kind: StatusComponentKind) -> (&'static str, &'static str) {
    match kind {
        StatusComponentKind::Summary => ("{summary}", "{summary}"),
        StatusComponentKind::Builds => (
            "{completed} of {expected} builds",
            "{completed}/{expected} builds",
        ),
        StatusComponentKind::Downloads => (
            "{completed} of {expected} downloads",
            "{completed}/{expected} dl",
        ),
        StatusComponentKind::Queries => {
            ("{completed} of {total} queries", "{completed}/{total} qry")
        }
        StatusComponentKind::Tasks => ("{completed} of {total} tasks", "{completed}/{total} tasks"),
        StatusComponentKind::Processes => {
            ("{running} of {total} processes", "{running}/{total} proc")
        }
        StatusComponentKind::Profiles => ("profiles: {profiles}", "{profiles}"),
        StatusComponentKind::Project => ("{name}", "{name}"),
        StatusComponentKind::Command => ("{command}", "{command}"),
        StatusComponentKind::Shell => ("{shell}", "{shell}"),
        StatusComponentKind::Elapsed => ("{elapsed}", "{elapsed}"),
        StatusComponentKind::Selected => ("{name} ({status})", "{name}"),
        StatusComponentKind::LogMode => ("{mode}", "{mode}"),
        StatusComponentKind::LogPosition => ("{current}/{total} ({percent}%)", "{current}/{total}"),
        StatusComponentKind::RetainedLogs => (
            "{retained} lines ({discarded} discarded)",
            "{retained}/{total}",
        ),
        StatusComponentKind::Search => ("/{query} {result}", "/{query}"),
        StatusComponentKind::Prompt => ("{prompt}", "{prompt}"),
        StatusComponentKind::PendingKey => ("{keys}", "{keys}"),
        StatusComponentKind::KeyHints => ("{hints}", "{hints}"),
        StatusComponentKind::Text => ("{text}", "{text}"),
    }
}

fn summary_text(summary: &ActivitySummary) -> String {
    let mut parts = Vec::new();
    let builds = summary.active_builds + summary.completed_builds + summary.failed_builds;
    if builds > 0 {
        let expected = summary
            .expected_builds
            .map(|value| value as usize)
            .unwrap_or(builds)
            .max(builds);
        parts.push(format!("{} of {expected} builds", summary.completed_builds));
    }
    let downloads = summary.active_downloads + summary.completed_downloads;
    if downloads > 0 {
        let expected = summary
            .expected_downloads
            .map(|value| value as usize)
            .unwrap_or(downloads)
            .max(downloads);
        parts.push(format!(
            "{} of {expected} downloads",
            summary.completed_downloads
        ));
    }
    let queries = summary.active_queries + summary.completed_queries;
    if queries > 0 {
        parts.push(format!(
            "{} of {queries} queries",
            summary.completed_queries
        ));
    }
    let tasks = summary.running_tasks + summary.completed_tasks + summary.failed_tasks;
    if tasks > 0 {
        parts.push(format!("{} of {tasks} tasks", summary.completed_tasks));
    }
    if summary.total_processes > 0 {
        parts.push(format!(
            "{} of {} {}",
            summary.running_processes,
            summary.total_processes,
            if summary.total_processes == 1 {
                "process"
            } else {
                "processes"
            }
        ));
    }
    parts.join(" │ ")
}

fn apply_format(format: &str, values: &BTreeMap<&'static str, String>) -> String {
    let mut output = format.replace("{{", "\u{0}").replace("}}", "\u{1}");
    for (name, value) in values {
        output = output.replace(&format!("{{{name}}}"), value);
    }
    output.replace('\u{0}', "{").replace('\u{1}', "}")
}

fn format_elapsed(started_at: std::time::Instant) -> String {
    let seconds = started_at.elapsed().as_secs();
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m{:02}s", seconds / 60, seconds % 60)
    } else {
        format!("{}h{:02}m", seconds / 3600, seconds % 3600 / 60)
    }
}

fn resolve_style(
    theme: &ThemeConfig,
    name: &str,
    component: &StatusComponentConfig,
) -> SegmentStyle {
    let mut style = SegmentStyle {
        foreground: match theme.preset {
            ThemePreset::Devenv => Some(match name {
                "key_hints" | "pending_key" => COLOR_INTERACTIVE,
                "summary" | "builds" | "downloads" | "queries" | "tasks" | "processes" => {
                    COLOR_COMPLETED
                }
                _ => Color::Reset,
            }),
            ThemePreset::Terminal => Some(match name {
                "key_hints" | "pending_key" => Color::Yellow,
                "summary" | "builds" | "downloads" | "queries" | "tasks" | "processes" => {
                    Color::Green
                }
                _ => Color::Reset,
            }),
            ThemePreset::None => None,
        },
        ..SegmentStyle::default()
    };
    for scope in ["statusline", &format!("statusline.{name}")] {
        if let Some(config) = theme.styles.get(scope) {
            apply_style(&mut style, theme, config);
        }
    }
    apply_style(&mut style, theme, &component.style);
    style
}

fn apply_style(
    resolved: &mut SegmentStyle,
    theme: &ThemeConfig,
    style: &crate::config::StyleConfig,
) {
    if let Some(color) = &style.foreground {
        resolved.foreground = theme.resolve_color(color).ok();
    }
    if let Some(color) = &style.background {
        resolved.background = theme.resolve_color(color).ok();
    }
    for modifier in &style.modifiers {
        match modifier {
            TextModifier::Bold => resolved.bold = true,
            TextModifier::Dim => resolved.dim = true,
            TextModifier::Italic => resolved.italic = true,
            TextModifier::Underline => resolved.underline = true,
            TextModifier::Reverse => resolved.reverse = true,
        }
    }
}

fn fit(candidates: &mut [Candidate], width: usize, separator: &str) {
    let mut order: Vec<usize> = (0..candidates.len()).collect();
    order.sort_by_key(|index| {
        (
            candidates[*index].required,
            candidates[*index].priority,
            *index,
        )
    });
    for index in &order {
        if total_width(candidates, separator) <= width {
            return;
        }
        if candidates[*index].content != candidates[*index].compact {
            candidates[*index].content = candidates[*index].compact.clone();
        }
    }
    for index in &order {
        if total_width(candidates, separator) <= width {
            return;
        }
        if !candidates[*index].required && candidates[*index].overflow == StatusOverflow::Hide {
            candidates[*index].content.clear();
        }
    }
    for index in &order {
        while total_width(candidates, separator) > width
            && UnicodeWidthStr::width(candidates[*index].content.as_str()) > 1
        {
            let current = UnicodeWidthStr::width(candidates[*index].content.as_str());
            candidates[*index].content = truncate(&candidates[*index].content, current - 1);
        }
    }
    while total_width(candidates, separator) > width {
        let Some(index) = order
            .iter()
            .copied()
            .find(|index| !candidates[*index].content.is_empty())
        else {
            break;
        };
        candidates[index].content.clear();
    }
}

fn total_width(candidates: &[Candidate], separator: &str) -> usize {
    let mut total = 0;
    let mut zones = 0usize;
    for zone in [Zone::Left, Zone::Center, Zone::Right] {
        let visible: Vec<_> = candidates
            .iter()
            .filter(|candidate| candidate.zone == zone && !candidate.content.is_empty())
            .collect();
        if !visible.is_empty() {
            zones += 1;
            total += visible
                .iter()
                .map(|candidate| UnicodeWidthStr::width(candidate.content.as_str()))
                .sum::<usize>();
            total += UnicodeWidthStr::width(separator) * visible.len().saturating_sub(1);
        }
    }
    total + zones.saturating_sub(1)
}

fn limit_width(value: String, width: Option<usize>) -> String {
    match width {
        Some(width) => truncate(&value, width),
        None => value,
    }
}

fn truncate(value: &str, width: usize) -> String {
    if UnicodeWidthStr::width(value) <= width {
        return value.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let ellipsis = if width > 1 { "…" } else { "" };
    let target = width.saturating_sub(UnicodeWidthStr::width(ellipsis));
    let mut output = String::new();
    let mut used = 0;
    for grapheme in value.graphemes(true) {
        let grapheme_width = UnicodeWidthStr::width(grapheme);
        if used + grapheme_width > target {
            break;
        }
        output.push_str(grapheme);
        used += grapheme_width;
    }
    output.push_str(ellipsis);
    output
}

pub fn separator_style(theme: &ThemeConfig) -> SegmentStyle {
    let mut style = SegmentStyle {
        foreground: match theme.preset {
            ThemePreset::Devenv => Some(COLOR_HIERARCHY),
            ThemePreset::Terminal => Some(Color::DarkGrey),
            ThemePreset::None => None,
        },
        ..SegmentStyle::default()
    };
    for scope in ["statusline", "statusline.separator"] {
        if let Some(config) = theme.styles.get(scope) {
            apply_style(&mut style, theme, config);
        }
    }
    style
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ColorSpec, TuiPreferences};
    use std::path::PathBuf;

    fn data() -> StatuslineData {
        StatuslineData {
            summary: ActivitySummary {
                completed_builds: 2,
                expected_builds: Some(3),
                total_builds: 3,
                ..ActivitySummary::default()
            },
            context: TuiRunContext {
                profiles: vec!["backend".to_string(), "debug".to_string()],
                project_root: Some(PathBuf::from("/work/forge")),
                command: Some("up".to_string()),
                ..TuiRunContext::default()
            },
            key_hints: Some("j/k navigate".to_string()),
            ..StatuslineData::default()
        }
    }

    #[test]
    fn log_position_percentage_handles_zero_and_nonzero_totals() {
        let mut statusline_data = StatuslineData {
            log_current: Some(1),
            log_total: Some(4),
            ..StatuslineData::default()
        };
        let config = StatusComponentConfig::default();

        let values = component_values(StatusComponentKind::LogPosition, &config, &statusline_data);
        assert_eq!(values["percent"], "25");

        statusline_data.log_total = Some(0);
        let values = component_values(StatusComponentKind::LogPosition, &config, &statusline_data);
        assert_eq!(values["percent"], "");
    }

    #[test]
    fn layouts_reorder_and_add_runtime_components() {
        let mut preferences = TuiPreferences::default();
        preferences.statusline.layouts.main.left = vec!["project".into(), "profiles".into()];
        preferences.statusline.layouts.main.right = vec!["command".into()];
        let rendered = render_statusline(StatuslineMode::Main, 120, &preferences, &data());
        assert_eq!(
            rendered
                .left
                .iter()
                .map(|segment| segment.content.as_str())
                .collect::<Vec<_>>(),
            ["forge", "profiles: backend,debug"]
        );
        assert_eq!(rendered.right[0].content, "up");
    }

    #[test]
    fn compact_formats_then_truncates_to_terminal_width() {
        let mut preferences = TuiPreferences::default();
        preferences.statusline.layouts.main = StatuslineLayout {
            left: vec!["profiles".into()],
            center: vec![],
            right: vec!["key_hints".into()],
        };
        let rendered = render_statusline(StatuslineMode::Main, 18, &preferences, &data());
        let width = rendered
            .left
            .iter()
            .chain(&rendered.center)
            .chain(&rendered.right)
            .map(|segment| UnicodeWidthStr::width(segment.content.as_str()))
            .sum::<usize>()
            + 1;
        assert!(width <= 18, "{rendered:?}");
    }

    #[test]
    fn required_components_are_truncated_after_optional_components() {
        let mut preferences = TuiPreferences::default();
        preferences.statusline.layouts.main = StatuslineLayout {
            left: vec!["anchor".into()],
            center: vec![],
            right: vec!["key_hints".into()],
        };
        preferences.statusline.components.insert(
            "anchor".into(),
            crate::config::StatusComponentConfig {
                kind: Some(StatusComponentKind::Text),
                text: Some("REQUIRED".into()),
                required: true,
                ..Default::default()
            },
        );
        let mut data = data();
        data.key_hints = Some("optional key hints".into());

        let rendered = render_statusline(StatuslineMode::Main, 16, &preferences, &data);

        assert_eq!(rendered.left[0].content, "REQUIRED");
    }

    #[test]
    fn component_palette_styles_are_resolved() {
        let mut preferences = TuiPreferences::default();
        preferences
            .theme
            .palette
            .insert("accent".into(), ColorSpec("#123456".into()));
        preferences.theme.styles.insert(
            "statusline.project".into(),
            crate::config::StyleConfig {
                foreground: Some(ColorSpec("accent".into())),
                ..crate::config::StyleConfig::default()
            },
        );
        preferences.statusline.layouts.main.left = vec!["project".into()];
        let rendered = render_statusline(StatuslineMode::Main, 80, &preferences, &data());
        assert_eq!(
            rendered.left[0].style.foreground,
            Some(Color::Rgb {
                r: 0x12,
                g: 0x34,
                b: 0x56
            })
        );
    }

    #[test]
    fn empty_runtime_components_are_hidden_by_default() {
        let mut preferences = TuiPreferences::default();
        preferences.statusline.layouts.main.left = vec!["profiles".into(), "project".into()];
        let rendered = render_statusline(
            StatuslineMode::Main,
            80,
            &preferences,
            &StatuslineData::default(),
        );
        assert!(rendered.left.is_empty());
    }

    #[test]
    fn custom_text_components_render_in_layout_order() {
        let mut preferences = TuiPreferences::default();
        preferences.statusline.layouts.main.left = vec!["first".into(), "second".into()];
        for (name, text) in [("first", "one"), ("second", "two")] {
            preferences.statusline.components.insert(
                name.to_string(),
                StatusComponentConfig {
                    kind: Some(StatusComponentKind::Text),
                    text: Some(text.to_string()),
                    ..StatusComponentConfig::default()
                },
            );
        }
        let rendered = render_statusline(
            StatuslineMode::Main,
            80,
            &preferences,
            &StatuslineData::default(),
        );
        assert_eq!(
            rendered
                .left
                .iter()
                .map(|segment| segment.content.as_str())
                .collect::<Vec<_>>(),
            ["one", "two"]
        );
    }

    #[test]
    fn disabled_statusline_keeps_interrupt_prompts_visible() {
        let mut preferences = TuiPreferences::default();
        preferences.statusline.enabled = false;
        let data = StatuslineData {
            prompt: Some("Quit devenv?".to_string()),
            key_hints: Some("Ctrl-C quit".to_string()),
            ..StatuslineData::default()
        };

        assert_eq!(
            render_statusline(StatuslineMode::Main, 80, &preferences, &data),
            RenderedStatusline::default()
        );
        let prompt = render_statusline(StatuslineMode::Prompt, 80, &preferences, &data);
        assert_eq!(prompt.left[0].content, "Quit devenv?");
        assert_eq!(prompt.right[0].content, "Ctrl-C quit");
    }

    #[test]
    fn interaction_fallbacks_survive_empty_and_retyped_layouts() {
        let data = StatuslineData {
            prompt: Some("Quit devenv?".to_string()),
            key_hints: Some("Ctrl-C quit".to_string()),
            ..StatuslineData::default()
        };
        let mut empty = TuiPreferences::default();
        empty.statusline.layouts.prompt = StatuslineLayout::default();

        let mut retyped = TuiPreferences::default();
        retyped.statusline.components.insert(
            "prompt".to_string(),
            StatusComponentConfig {
                kind: Some(StatusComponentKind::Text),
                text: Some("custom".to_string()),
                ..StatusComponentConfig::default()
            },
        );

        for preferences in [empty, retyped] {
            let rendered = render_statusline(StatuslineMode::Prompt, 80, &preferences, &data);
            let content = rendered
                .left
                .iter()
                .chain(&rendered.center)
                .chain(&rendered.right)
                .map(|segment| segment.content.as_str())
                .collect::<Vec<_>>();
            assert!(content.contains(&"Quit devenv?"), "{content:?}");
            assert!(content.contains(&"Ctrl-C quit"), "{content:?}");
        }
    }

    #[test]
    fn disabled_statusline_keeps_search_visible() {
        let mut preferences = TuiPreferences::default();
        preferences.statusline.enabled = false;
        preferences.statusline.layouts.search = StatuslineLayout::default();
        let data = StatuslineData {
            search_query: Some("api".to_string()),
            search_result: Some("1 match".to_string()),
            key_hints: Some("Enter select".to_string()),
            ..StatuslineData::default()
        };

        let rendered = render_statusline(StatuslineMode::Search, 80, &preferences, &data);
        assert_eq!(rendered.left[0].content, "/api 1 match");
        assert_eq!(rendered.right[0].content, "Enter select");
    }

    #[test]
    fn empty_search_query_does_not_duplicate_the_search_component() {
        let data = StatuslineData {
            search_query: Some(String::new()),
            search_result: Some("1 match".to_string()),
            ..StatuslineData::default()
        };

        let defaults = TuiPreferences::default();
        let rendered = render_statusline(StatuslineMode::Search, 80, &defaults, &data);
        assert_eq!(
            rendered
                .left
                .iter()
                .chain(&rendered.center)
                .chain(&rendered.right)
                .filter(|segment| segment.content == "/ 1 match")
                .count(),
            1
        );

        let mut hidden = TuiPreferences::default();
        hidden.statusline.components.insert(
            "search".to_string(),
            StatusComponentConfig {
                format: Some("searching".to_string()),
                compact_format: Some("searching".to_string()),
                ..StatusComponentConfig::default()
            },
        );
        let rendered = render_statusline(StatuslineMode::Search, 80, &hidden, &data);
        let content = rendered
            .left
            .iter()
            .chain(&rendered.center)
            .chain(&rendered.right)
            .map(|segment| segment.content.as_str())
            .collect::<Vec<_>>();
        assert!(content.contains(&"searching"), "{content:?}");
        assert!(content.contains(&"/ 1 match"), "{content:?}");
    }

    #[test]
    fn selected_status_uses_the_underlying_lifecycle() {
        let activity = |variant, state| Activity {
            id: 1,
            name: "item".to_string(),
            short_name: "item".to_string(),
            parent_id: None,
            start_time: std::time::Instant::now(),
            state,
            completed_at: None,
            detail: None,
            variant,
            progress: None,
            details: Vec::new(),
            level: devenv_activity::ActivityLevel::Info,
        };
        let process = activity(
            ActivityVariant::Process(crate::model::ProcessActivity {
                status: ProcessStatus::GaveUp,
                ports: Vec::new(),
                urls: Vec::new(),
                ready_probe: None,
            }),
            NixActivityState::Active,
        );
        let task = activity(
            ActivityVariant::Task(crate::model::TaskActivity {
                status: TaskDisplayStatus::Failed,
                duration: None,
                show_output: false,
                last_log_line: None,
            }),
            NixActivityState::Active,
        );
        let nix = activity(
            ActivityVariant::Devenv,
            NixActivityState::Completed {
                success: false,
                cached: false,
                duration: std::time::Duration::ZERO,
            },
        );

        assert_eq!(selected_status(&process), "gave up");
        assert_eq!(selected_status(&task), "failed");
        assert_eq!(selected_status(&nix), "failed");
    }

    #[test]
    fn truncation_preserves_grapheme_clusters() {
        assert_eq!(truncate("e\u{301}xy", 2), "e\u{301}…");
        assert_eq!(truncate("A👨‍👩‍👧‍👦BC", 4), "A👨‍👩‍👧‍👦…");
    }

    #[test]
    fn interrupt_prompt_hints_only_show_available_actions() {
        let preferences = TuiPreferences::default();
        let keymap = preferences.keybindings.resolve().unwrap();

        let attached = interrupt_prompt_key_hints(&keymap, true, 160);
        assert!(attached.contains("c cancel"));
        assert!(attached.contains("s stop manager"));
        assert!(attached.contains("Ctrl-C detach"));
        assert!(!attached.contains("q quit"));

        let starting = interrupt_prompt_key_hints(&keymap, false, 160);
        assert!(starting.contains("c cancel"));
        assert!(starting.contains("q quit"));
        assert!(starting.contains("Ctrl-C quit"));
        assert!(!starting.contains("s stop manager"));

        assert_eq!(
            interrupt_prompt_key_hints(&keymap, true, 80),
            "c:cancel s:stop ^C:detach"
        );
        assert_eq!(
            interrupt_prompt_key_hints(&keymap, false, 80),
            "c:cancel q:quit ^C:quit"
        );
    }

    #[test]
    fn separator_style_inherits_global_statusline_style() {
        let mut theme = ThemeConfig::default();
        theme.styles.insert(
            "statusline".to_string(),
            crate::config::StyleConfig {
                foreground: Some(ColorSpec("red".to_string())),
                background: Some(ColorSpec("blue".to_string())),
                ..crate::config::StyleConfig::default()
            },
        );
        theme.styles.insert(
            "statusline.separator".to_string(),
            crate::config::StyleConfig {
                foreground: Some(ColorSpec("yellow".to_string())),
                ..crate::config::StyleConfig::default()
            },
        );

        let style = separator_style(&theme);
        assert_eq!(style.foreground, Some(Color::Yellow));
        assert_eq!(style.background, Some(Color::Blue));
    }
}
