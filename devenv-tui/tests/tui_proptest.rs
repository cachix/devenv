#![cfg(feature = "test-all")]
//! Property-based tests for the TUI.
//!
//! Where `tui_tests.rs` pins exact rendered output for fixed inputs, this suite
//! attacks the *state machine*: it generates random streams of activity events
//! (every activity type, with adversarial unicode/control-character payloads)
//! interleaved with random UI commands (navigation, filtering, expand/back,
//! interrupt prompt, and resizes down to degenerate sizes), then asserts the
//! invariants that must hold no matter what:
//!
//! 1. `view()` must render without panicking at any terminal size.
//! 2. Model query methods must never panic on any (possibly inconsistent) state.
//! 3. `select_activity` always lands inside the selectable set.
//! 4. The total log-line counter never undercounts the buffered lines.
//! 5. Rendered lines never exceed the terminal width (no overflow / bad wrap).
//!
//! Determinism: the `test-all` feature pulls in `deterministic-tui`, so spinner
//! frames and elapsed times render as fixed placeholders and a failing case
//! shrinks to a stable, reproducible minimal sequence.

use devenv_activity::test_helpers::*;
use devenv_activity::{ActivityLevel, ActivityOutcome, FetchKind, ProcessStatus};
use devenv_tui::view::view;
use devenv_tui::{ActivityModel, RenderContext, UiState, ViewMode};
use iocraft::prelude::*;
use proptest::prelude::*;
use unicode_width::UnicodeWidthStr;

/// Activity ids are drawn from a small space so that events frequently target
/// the same activity (parents, logs, completions) instead of always creating
/// fresh orphans.
const MAX_ID: u64 = 8;

// ---------------------------------------------------------------------------
// Strategies for the leaf values shared across event kinds.
// ---------------------------------------------------------------------------

/// Strings designed to break naive rendering / width / slicing logic.
fn nasty_string() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(String::new()),
        Just("hello world".to_string()),
        Just("你好世界".to_string()),             // wide (CJK) glyphs
        Just("🎉🚀👨‍👩‍👧‍👦".to_string()),               // emoji + ZWJ sequence
        Just("\u{200b}\u{200b}zero".to_string()), // zero-width
        Just("\x1b[31mfake-ansi\x1b[0m".to_string()), // embedded escape
        Just("carriage\rreturn".to_string()),
        Just("new\nline\r\nbreaks".to_string()),
        Just("\ttabbed\tcontent".to_string()),
        Just("\0nul\0byte".to_string()),
        Just("a".repeat(400)),             // very long single line
        Just("مرحبا بالعالم".to_string()), // RTL
        "[a-zA-Z0-9 ._/-]{0,48}",
        "\\PC{0,24}", // arbitrary non-control unicode
    ]
}

fn outcome() -> impl Strategy<Value = ActivityOutcome> {
    prop_oneof![
        Just(ActivityOutcome::Success),
        Just(ActivityOutcome::Failed),
        Just(ActivityOutcome::Cancelled),
        Just(ActivityOutcome::Cached),
        Just(ActivityOutcome::Skipped),
        Just(ActivityOutcome::DependencyFailed),
    ]
}

fn level() -> impl Strategy<Value = ActivityLevel> {
    prop_oneof![
        Just(ActivityLevel::Error),
        Just(ActivityLevel::Warn),
        Just(ActivityLevel::Info),
        Just(ActivityLevel::Debug),
        Just(ActivityLevel::Trace),
    ]
}

fn fetch_kind() -> impl Strategy<Value = FetchKind> {
    prop_oneof![
        Just(FetchKind::Download),
        Just(FetchKind::Query),
        Just(FetchKind::Tree),
        Just(FetchKind::Copy),
    ]
}

fn proc_status_val() -> impl Strategy<Value = ProcessStatus> {
    prop_oneof![
        Just(ProcessStatus::NotStarted),
        Just(ProcessStatus::Waiting),
        Just(ProcessStatus::Starting),
        Just(ProcessStatus::Running),
        Just(ProcessStatus::Ready),
        Just(ProcessStatus::Restarting),
        Just(ProcessStatus::Stopping),
        Just(ProcessStatus::Stopped),
        Just(ProcessStatus::Exited),
        Just(ProcessStatus::GaveUp),
    ]
}

fn id() -> impl Strategy<Value = u64> {
    0..MAX_ID
}

fn parent() -> impl Strategy<Value = Option<u64>> {
    prop_oneof![Just(None), (0..MAX_ID).prop_map(Some)]
}

// ---------------------------------------------------------------------------
// EvtSpec: a serializable description of one activity event, turned into a real
// `ActivityEvent` via the test helpers at apply time. Kept as a plain enum so
// proptest can cheaply clone/shrink it regardless of `ActivityEvent`'s traits.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum EvtSpec {
    BuildStart(u64, Option<u64>, String),
    BuildPhase(u64, String),
    BuildLog(u64, String, bool),
    BuildComplete(u64, ActivityOutcome),
    FetchStart(u64, FetchKind, String),
    FetchProgress(u64, u64, Option<u64>),
    FetchComplete(u64, ActivityOutcome),
    TaskSingle(u64, String, Option<u64>, bool, bool),
    TaskStart(u64),
    TaskLog(u64, String, bool),
    TaskProgress(u64, u64, u64),
    TaskComplete(u64, ActivityOutcome),
    ProcessStart(u64, String),
    ProcessStatus(u64, ProcessStatus),
    ProcessComplete(u64, ActivityOutcome),
    OperationStart(u64, String),
    OperationLog(u64, String, bool),
    OperationProgress(u64, u64, u64),
    OperationComplete(u64, ActivityOutcome),
    EvalStart(u64, String, ActivityLevel),
    EvalLog(u64, String),
    EvalComplete(u64, ActivityOutcome),
    CommandStart(u64, String),
    CommandLog(u64, String, bool),
    CommandComplete(u64, ActivityOutcome),
    Msg(ActivityLevel, String),
}

fn evt_spec() -> impl Strategy<Value = EvtSpec> {
    prop_oneof![
        (id(), parent(), nasty_string()).prop_map(|(i, p, n)| EvtSpec::BuildStart(i, p, n)),
        (id(), nasty_string()).prop_map(|(i, n)| EvtSpec::BuildPhase(i, n)),
        (id(), nasty_string(), any::<bool>()).prop_map(|(i, l, e)| EvtSpec::BuildLog(i, l, e)),
        (id(), outcome()).prop_map(|(i, o)| EvtSpec::BuildComplete(i, o)),
        (id(), fetch_kind(), nasty_string()).prop_map(|(i, k, n)| EvtSpec::FetchStart(i, k, n)),
        (id(), any::<u64>(), proptest::option::of(any::<u64>()))
            .prop_map(|(i, c, t)| EvtSpec::FetchProgress(i, c, t)),
        (id(), outcome()).prop_map(|(i, o)| EvtSpec::FetchComplete(i, o)),
        (id(), nasty_string(), parent(), any::<bool>(), any::<bool>())
            .prop_map(|(i, n, p, s, pr)| EvtSpec::TaskSingle(i, n, p, s, pr)),
        id().prop_map(EvtSpec::TaskStart),
        (id(), nasty_string(), any::<bool>()).prop_map(|(i, l, e)| EvtSpec::TaskLog(i, l, e)),
        (id(), any::<u64>(), any::<u64>()).prop_map(|(i, d, e)| EvtSpec::TaskProgress(i, d, e)),
        (id(), outcome()).prop_map(|(i, o)| EvtSpec::TaskComplete(i, o)),
        (id(), nasty_string()).prop_map(|(i, n)| EvtSpec::ProcessStart(i, n)),
        (id(), proc_status_val()).prop_map(|(i, s)| EvtSpec::ProcessStatus(i, s)),
        (id(), outcome()).prop_map(|(i, o)| EvtSpec::ProcessComplete(i, o)),
        (id(), nasty_string()).prop_map(|(i, n)| EvtSpec::OperationStart(i, n)),
        (id(), nasty_string(), any::<bool>()).prop_map(|(i, l, e)| EvtSpec::OperationLog(i, l, e)),
        (id(), any::<u64>(), any::<u64>())
            .prop_map(|(i, d, e)| EvtSpec::OperationProgress(i, d, e)),
        (id(), outcome()).prop_map(|(i, o)| EvtSpec::OperationComplete(i, o)),
        (id(), nasty_string(), level()).prop_map(|(i, n, l)| EvtSpec::EvalStart(i, n, l)),
        (id(), nasty_string()).prop_map(|(i, l)| EvtSpec::EvalLog(i, l)),
        (id(), outcome()).prop_map(|(i, o)| EvtSpec::EvalComplete(i, o)),
        (id(), nasty_string()).prop_map(|(i, n)| EvtSpec::CommandStart(i, n)),
        (id(), nasty_string(), any::<bool>()).prop_map(|(i, l, e)| EvtSpec::CommandLog(i, l, e)),
        (id(), outcome()).prop_map(|(i, o)| EvtSpec::CommandComplete(i, o)),
        (level(), nasty_string()).prop_map(|(l, t)| EvtSpec::Msg(l, t)),
    ]
}

fn build_event(spec: EvtSpec) -> devenv_activity::ActivityEvent {
    match spec {
        EvtSpec::BuildStart(i, p, n) => build_start_with(i, n, p),
        EvtSpec::BuildPhase(i, n) => build_phase(i, n),
        EvtSpec::BuildLog(i, l, e) => build_log(i, l, e),
        EvtSpec::BuildComplete(i, o) => build_complete(i, o),
        EvtSpec::FetchStart(i, k, n) => fetch_start(i, k, n),
        EvtSpec::FetchProgress(i, c, t) => fetch_progress(i, c, t),
        EvtSpec::FetchComplete(i, o) => fetch_complete(i, o),
        EvtSpec::TaskSingle(i, n, p, s, pr) => task_hierarchy_single(i, n, p, s, pr),
        EvtSpec::TaskStart(i) => task_start(i),
        EvtSpec::TaskLog(i, l, e) => task_log(i, l, e),
        EvtSpec::TaskProgress(i, d, e) => task_progress(i, d, e),
        EvtSpec::TaskComplete(i, o) => task_complete(i, o),
        EvtSpec::ProcessStart(i, n) => process_start(i, n),
        EvtSpec::ProcessStatus(i, s) => process_status(i, s),
        EvtSpec::ProcessComplete(i, o) => process_complete(i, o),
        EvtSpec::OperationStart(i, n) => operation_start(i, n),
        EvtSpec::OperationLog(i, l, e) => operation_log(i, l, e),
        EvtSpec::OperationProgress(i, d, e) => operation_progress(i, d, e),
        EvtSpec::OperationComplete(i, o) => operation_complete(i, o),
        EvtSpec::EvalStart(i, n, l) => evaluate_start(i, n, l),
        EvtSpec::EvalLog(i, l) => evaluate_log(i, l),
        EvtSpec::EvalComplete(i, o) => evaluate_complete(i, o),
        EvtSpec::CommandStart(i, n) => command_start(i, n),
        EvtSpec::CommandLog(i, l, e) => command_log(i, l, e),
        EvtSpec::CommandComplete(i, o) => command_complete(i, o),
        EvtSpec::Msg(l, t) => message(l, t),
    }
}

// ---------------------------------------------------------------------------
// Ops: the full alphabet a session can perform — data events plus the UI
// commands the key handlers (app.rs) issue against UiState.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Op {
    Event(EvtSpec),
    SelectDown,
    SelectUp,
    ToggleHide,
    Expand,
    Back,
    ShowInterrupt,
    ClearInterrupt,
    Resize(u16, u16),
}

/// Sizes weighted toward degenerate extremes (1-cell, single-row, single-column)
/// where off-by-one and divide-by-width bugs live, plus a broad random range.
fn size() -> impl Strategy<Value = (u16, u16)> {
    prop_oneof![
        Just((1u16, 1u16)),
        Just((2u16, 1u16)),
        Just((1u16, 24u16)),
        Just((3u16, 2u16)),
        Just((10u16, 1u16)),
        Just((200u16, 1u16)),
        Just((1u16, 80u16)),
        (1u16..=220u16, 1u16..=80u16),
    ]
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        20 => evt_spec().prop_map(Op::Event),
        4 => Just(Op::SelectDown),
        4 => Just(Op::SelectUp),
        2 => Just(Op::ToggleHide),
        2 => Just(Op::Expand),
        2 => Just(Op::Back),
        1 => Just(Op::ShowInterrupt),
        1 => Just(Op::ClearInterrupt),
        4 => size().prop_map(|(w, h)| Op::Resize(w, h)),
    ]
}

/// Like [`op`], but resizes stay within usable terminal widths (>= 40). Below
/// ~30 columns iocraft itself can no longer fit content, so width-fit is only a
/// meaningful guarantee on terminals a human would actually use.
fn op_wide() -> impl Strategy<Value = Op> {
    prop_oneof![
        20 => evt_spec().prop_map(Op::Event),
        4 => Just(Op::SelectDown),
        4 => Just(Op::SelectUp),
        2 => Just(Op::ToggleHide),
        2 => Just(Op::Expand),
        2 => Just(Op::Back),
        2 => Just(Op::ShowInterrupt),
        1 => Just(Op::ClearInterrupt),
        4 => (40u16..=300u16, 1u16..=80u16).prop_map(|(w, h)| Op::Resize(w, h)),
    ]
}

fn apply_op(model: &mut ActivityModel, ui: &mut UiState, op: Op) {
    match op {
        Op::Event(spec) => {
            model.apply_activity_event(build_event(spec));
        }
        Op::SelectDown => {
            let selectable = model.get_selectable_activity_ids(ui);
            ui.select_activity(&selectable, true);
        }
        Op::SelectUp => {
            let selectable = model.get_selectable_activity_ids(ui);
            ui.select_activity(&selectable, false);
        }
        Op::ToggleHide => ui.toggle_hide_stopped_processes(),
        Op::Expand => {
            let selectable = model.get_selectable_activity_ids(ui);
            let target = ui
                .selected_activity
                .or_else(|| selectable.first().copied())
                .unwrap_or(0);
            ui.view_mode = ViewMode::ExpandedLogs {
                activity_id: target,
            };
        }
        Op::Back => {
            ui.selected_activity = None;
            ui.view_mode = ViewMode::Main;
        }
        Op::ShowInterrupt => ui.show_interrupt_prompt(false),
        Op::ClearInterrupt => ui.clear_interrupt_prompt(),
        Op::Resize(w, h) => ui.set_terminal_size(w, h),
    }
}

/// Render the main view and return the plain-text output (no ANSI under
/// `deterministic-tui`). Panicking here fails the property — that is the point.
fn render(model: &ActivityModel, ui: &UiState) -> String {
    let width = ui.terminal_size.width.max(1) as usize;
    let child: AnyElement = view(model, ui, RenderContext::Normal, None, false).into();
    // Production mounts the TUI below an explicitly sized terminal viewport.
    // Mirror that boundary so percentage widths and overflow are resolved by
    // iocraft instead of laying out an unconstrained tree at intrinsic width.
    let mut element = element! {
        View(width: width as u32) {
            #(vec![child])
        }
    };
    element.render(Some(width)).to_string()
}

fn check_invariants(model: &ActivityModel, ui: &UiState) -> Result<(), TestCaseError> {
    // (2) Query methods must not panic on any state. Calling them is the assertion.
    let _ = model.calculate_summary();
    let _ = model.get_active_activities();
    let _ = model.get_display_activities(ui);
    let _ = model.get_error_messages();
    let _ = model.get_total_duration();
    let selectable = model.get_selectable_activity_ids(ui);

    // (3) Anything in the selectable set must resolve to a real activity.
    for sel_id in &selectable {
        prop_assert!(
            model.get_activity(*sel_id).is_some(),
            "selectable id {sel_id} has no backing activity"
        );
    }

    // (4) The total log counter must never undercount the live buffer.
    for i in 0..MAX_ID {
        if let Some(logs) = model.get_build_logs(i) {
            prop_assert!(
                model.get_log_line_count(i) >= logs.len(),
                "log_line_count({i})={} < buffered {}",
                model.get_log_line_count(i),
                logs.len()
            );
        }
    }

    // (1) Rendering must not panic. (Width-fit is checked separately, see
    // `rendered_lines_fit_width` — the codebase does not yet guarantee it.)
    let _ = render(model, ui);
    Ok(())
}

/// Largest display-column width of any rendered line.
fn max_line_width(out: &str) -> usize {
    out.lines().map(UnicodeWidthStr::width).max().unwrap_or(0)
}

/// A selected process keeps complete controls at every usable width and keeps
/// its status summary whenever both can fit without fragmentation.
#[test]
fn selected_navbar_preserves_status_and_fits_usable_widths() {
    let mut m = ActivityModel::new();
    m.apply_activity_event(process_start(1, "web"));
    m.apply_activity_event(process_status(1, ProcessStatus::Running));
    // A stopped process makes the hide/show-stopped toggle appear (worst case).
    m.apply_activity_event(process_start(2, "db"));
    m.apply_activity_event(process_complete(2, ActivityOutcome::Success));

    for w in 40u16..=240 {
        let mut ui = UiState::new();
        ui.set_terminal_size(w, 24);
        ui.selected_activity = Some(1);
        let out = render(&m, &ui);
        let widest = max_line_width(&out);
        assert!(
            widest <= w as usize,
            "nav bar width {widest} exceeds terminal width {w}:\n{out}"
        );
        if w < 60 {
            assert!(
                out.contains("↑↓ j/k ^D/^U Enter ^E ^X ^R ^H / Esc"),
                "ultra-compact controls are incomplete at width {w}:\n{out}"
            );
        } else if w < 120 {
            assert!(
                out.contains("↑↓ j/k ^D/^U • Enter ▾ • ^E ▼ • ^X ^R ^H / Esc clear"),
                "symbol controls are incomplete at width {w}:\n{out}"
            );
        } else if w < 200 {
            assert!(
                out.contains(
                    "↑↓ j/k ^D/^U nav Enter preview ^E logs ^X stop ^R restart ^H hide / search Esc clear"
                ),
                "compact controls are incomplete at width {w}:\n{out}"
            );
        } else {
            assert!(
                out.contains(
                    "↑↓ j/k Ctrl-D/Ctrl-U navigate • Enter/l/→ preview logs • Ctrl-E full logs • Ctrl-X stop process • Ctrl-R (re)start process • Ctrl-H hide stopped • / search processes • Esc clear"
                ),
                "wide controls are incomplete at width {w}:\n{out}"
            );
        }
        if w >= 72 {
            let status = if w < 120 {
                "1/2 processes"
            } else {
                "1 of 2 processes"
            };
            assert!(
                out.contains(status),
                "selected footer lost process status at width {w}:\n{out}"
            );
        }
    }
}

#[test]
fn selected_navbar_fits_mixed_summary_without_fragmenting_controls() {
    let mut model = ActivityModel::new();
    model.apply_activity_event(build_start_with(1, "build", None));
    model.apply_activity_event(build_complete(1, ActivityOutcome::Success));
    model.apply_activity_event(fetch_start(2, FetchKind::Download, "download"));
    model.apply_activity_event(fetch_complete(2, ActivityOutcome::Success));
    model.apply_activity_event(fetch_start(3, FetchKind::Query, "query"));
    model.apply_activity_event(fetch_complete(3, ActivityOutcome::Success));
    model.apply_activity_event(task_hierarchy_single(4, "task", None, true, false));
    model.apply_activity_event(task_start(4));
    model.apply_activity_event(task_complete(4, ActivityOutcome::Success));
    model.apply_activity_event(process_start(5, "process"));
    model.apply_activity_event(process_status(5, ProcessStatus::Ready));

    for width in 40u16..=240 {
        let mut ui = UiState::new();
        ui.set_terminal_size(width, 24);
        ui.selected_activity = Some(5);
        let output = render(&model, &ui);
        let footer = output.lines().find(|line| line.contains('↑')).unwrap();
        let summary = footer[..footer.find('↑').unwrap()].trim();

        assert!(max_line_width(&output) <= usize::from(width), "{output}");
        assert!(
            summary.is_empty()
                || summary == "1/1 builds │ 1/1 downloads │ 1/1 queries │ 1/1 tasks │ 1 process"
                || summary
                    == "1 of 1 builds  │  1 of 1 downloads  │  1 of 1 queries  │  1 of 1 tasks  │  1 process",
            "width {width}: {footer:?}"
        );
        assert!(footer.contains("↑↓ j/k"), "width {width}: {footer:?}");
        assert!(
            footer.contains("^D/^U") || footer.contains("Ctrl-D/Ctrl-U"),
            "width {width}: {footer:?}"
        );
        assert!(footer.contains("Enter"), "width {width}: {footer:?}");
        assert!(
            footer.contains("^E") || footer.contains("Ctrl-E"),
            "width {width}: {footer:?}"
        );
        assert!(
            footer.contains("^X") || footer.contains("Ctrl-X"),
            "width {width}: {footer:?}"
        );
        assert!(
            footer.contains("^R") || footer.contains("Ctrl-R"),
            "width {width}: {footer:?}"
        );
        assert!(footer.contains("Esc"), "width {width}: {footer:?}");

        if width == 140 {
            assert!(
                footer.contains("↑↓ j/k ^D/^U • Enter ▾ • ^E ▼ • ^X ^R / Esc clear"),
                "{footer:?}"
            );
        }
    }
}

/// A mixed summary plus the show-stopped hint must stay within the flex row at
/// every usable width. The framework clips lower-priority summary details while
/// preserving the controls on the right.
#[test]
fn navbar_fits_mixed_summary_widths() {
    let mut model = ActivityModel::new();
    model.apply_activity_event(process_start(1, "process"));
    model.apply_activity_event(build_start_with(2, "build", None));
    model.apply_activity_event(process_complete(1, ActivityOutcome::Success));
    model.apply_activity_event(fetch_start(3, FetchKind::Download, "download"));

    for width in 40u16..=120 {
        let mut ui = UiState::new();
        ui.set_terminal_size(width, 1);

        let output = render(&model, &ui);
        let widest = max_line_width(&output);
        assert!(
            widest <= width as usize,
            "nav bar width {widest} exceeds terminal width {width}:\n{output}"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 128,
        max_shrink_iters: 4096,
        ..ProptestConfig::default()
    })]

    /// The headline property: a TUI driven by any sequence of data events and
    /// UI commands at any size never panics and preserves the core invariants.
    #[test]
    fn tui_survives_random_sessions(ops in prop::collection::vec(op(), 0..160)) {
        let mut model = ActivityModel::new();
        let mut ui = UiState::new();
        ui.set_terminal_size(80, 24);

        for op in ops {
            apply_op(&mut model, &mut ui, op);
            check_invariants(&model, &ui)?;
        }
    }

    /// `select_activity` contract: against a non-empty selectable set, the
    /// resulting selection is always a member of that set, regardless of the
    /// starting selection (stale ids included).
    #[test]
    fn select_activity_stays_in_set(
        set in prop::collection::vec(0u64..16, 1..12),
        start in proptest::option::of(0u64..20),
        forward in any::<bool>(),
    ) {
        // De-dup while preserving order, mirroring how the model builds the list.
        let mut seen = std::collections::HashSet::new();
        let selectable: Vec<u64> = set.into_iter().filter(|x| seen.insert(*x)).collect();

        let mut ui = UiState::new();
        ui.selected_activity = start;
        ui.select_activity(&selectable, forward);

        let selected = ui.selected_activity.expect("non-empty set must yield a selection");
        prop_assert!(
            selectable.contains(&selected),
            "selection {selected} not in selectable set {selectable:?}"
        );
    }

    /// `select_activity` against an empty set never selects and never panics.
    #[test]
    fn select_activity_empty_is_noop(start in proptest::option::of(0u64..20), forward in any::<bool>()) {
        let mut ui = UiState::new();
        ui.selected_activity = start;
        ui.select_activity(&[], forward);
        prop_assert_eq!(ui.selected_activity, start);
    }

    /// (5) On usable terminals (>= 40 columns) no rendered line ever exceeds the
    /// terminal width. The help row has a real width budget and clips its least
    /// important trailing hints when the full vocabulary does not fit.
    #[test]
    fn rendered_lines_fit_usable_width(ops in prop::collection::vec(op_wide(), 0..120)) {
        let mut model = ActivityModel::new();
        let mut ui = UiState::new();
        ui.set_terminal_size(80, 24);

        for op in ops {
            apply_op(&mut model, &mut ui, op);
            let width = ui.terminal_size.width.max(1) as usize;
            let out = render(&model, &ui);
            let widest = max_line_width(&out);
            prop_assert!(
                widest <= width,
                "rendered line width {widest} exceeds terminal width {width}\n{out}"
            );
        }
    }
}

/// Deterministic regression for the interrupt (quit) prompt overflow: the prompt
/// row must fit every realistic terminal width. (Found by the property tests:
/// the prompt previously rendered 82 columns wide on an 80-column terminal.)
#[test]
fn interrupt_prompt_fits_usable_widths() {
    let model = populated_model();
    for w in 40u16..=200 {
        let mut ui = UiState::new();
        ui.set_terminal_size(w, 24);
        ui.show_interrupt_prompt(false);
        let out = render(&model, &ui);
        let widest = max_line_width(&out);
        assert!(
            widest <= w as usize,
            "interrupt prompt width {widest} exceeds terminal width {w}:\n{out}"
        );
    }
}

// ---------------------------------------------------------------------------
// Explicit degenerate-size regression smoke tests (deterministic, no shrinking
// needed): a populated model must render at extreme sizes without panicking.
// ---------------------------------------------------------------------------

fn populated_model() -> ActivityModel {
    let mut m = ActivityModel::new();
    m.apply_activity_event(build_start_with(1, "build 你好", None));
    m.apply_activity_event(build_phase(1, "buildPhase"));
    m.apply_activity_event(build_log(1, "🎉 emoji log line that is fairly long", false));
    m.apply_activity_event(fetch_start(2, FetchKind::Download, "nixpkgs"));
    m.apply_activity_event(fetch_progress(2, 5000, Some(10000)));
    m.apply_activity_event(task_hierarchy_single(3, "task", None, true, false));
    m.apply_activity_event(task_start(3));
    m.apply_activity_event(task_log(3, "task output", false));
    m.apply_activity_event(message(
        ActivityLevel::Error,
        "something failed\nwith detail",
    ));
    m
}

#[test]
fn renders_at_degenerate_sizes_without_panic() {
    let model = populated_model();
    let sizes = [
        (1u16, 1u16),
        (2, 1),
        (1, 2),
        (3, 3),
        (10, 1),
        (1, 40),
        (200, 1),
        (40, 200),
        (300, 100),
    ];
    for (w, h) in sizes {
        let mut ui = UiState::new();
        ui.set_terminal_size(w, h);
        // Exercise both view modes and the interrupt overlay.
        let mut element: AnyElement = view(&model, &ui, RenderContext::Normal, None, false).into();
        let _ = element.render(Some(w.max(1) as usize)).to_string();

        ui.show_interrupt_prompt(false);
        let mut element: AnyElement = view(&model, &ui, RenderContext::Final, None, true).into();
        let _ = element.render(Some(w.max(1) as usize)).to_string();
    }
}
