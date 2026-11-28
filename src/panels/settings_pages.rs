//! The Settings pages that list things to act on: Projects, and Harnesses (Claude Code, Codex, OpenCode, Antigravity)
//! with shared model and subagent controls and provider-specific configuration.

use gpui::{
    AnyElement, ClickEvent, Context, Div, FontWeight, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, Window, div, px,
};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::assets::IconName;
use crate::panels::widgets::icon;

use crate::app::{Act, Lookup, Modal, Workspace, WorkspacePicker};
use crate::harness::Harness;
use crate::harness::subagents::AgentModel;
use crate::panels::logos;
use crate::panels::theme::theme;
use crate::panels::ui::switch::switch;
use crate::panels::ui::button as btn;
use crate::panels::ui::label::section_label;

fn faint(text: impl Into<SharedString>) -> Div {
    div().text_size(crate::panels::scale::px(11.0)).text_color(theme().subtle_foreground).child(text.into())
}

fn warn(text: impl Into<SharedString>) -> Div {
    div().text_size(crate::panels::scale::px(11.0)).text_color(theme().warning).child(text.into())
}

/// A flat Settings section with a quiet heading and separated rows.
pub fn settings_group(title: impl Into<SharedString>, trailing: Option<AnyElement>, rows: Vec<AnyElement>) -> Div {
    let header = div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(8.0))
        .child(section_label(title.into().to_string().to_uppercase()))
        .children(trailing);
    let mut card = div().flex().flex_col().w_full();
    for (i, row) in rows.into_iter().enumerate() {
        if i > 0 {
            card = card.child(crate::panels::ui::separator::horizontal());
        }
        card = card.child(row);
    }
    div().flex().flex_col().gap(px(2.0)).w_full().child(header.px(px(14.0))).child(card)
}

/// An On/Off row that runs `flip` on the workspace when clicked, with an optional faint `tag` after the label.
pub fn switch_row(
    id: impl Into<gpui::ElementId>,
    label: impl Into<SharedString>,
    tag: Option<SharedString>,
    on: bool,
    flip: impl Fn(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static,
    cx: &mut Context<Workspace>,
) -> gpui_kit::base::Button {
    crate::panels::ui::switch::row(id)
        .child(
            div().flex().flex_col().flex_1().min_w_0().gap(px(3.0))
                .child(label.into())
                .children(tag.map(faint)),
        )
        .child(switch(on))
        .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
            flip(ws, window, cx);
            cx.notify();
        }))
}

/// The checklist reads the adapter declarations used by the action guards; no per-harness feature table.
fn adapter_feature_rows(harness: Harness) -> Vec<(&'static str, bool, &'static str, Option<&'static str>)> {
    use crate::harness::capabilities::{Capability, Initiator};
    let adapter = harness.adapter();
    let caps = adapter.capabilities;
    let declared = [
        ("Rewind messages", caps.rewind),
        ("Fork conversations", caps.fork),
        ("Continue in a fresh session", caps.continue_fresh),
        ("Client compaction fallback", caps.compaction_fallback),
        ("Background text generation", caps.text_generation),
        ("Receive a handoff", caps.handoff_in.allows(Initiator::User)),
        ("Hand off to another harness", caps.handoff_out.allows(Initiator::User)),
        ("Automatic incoming handoff", caps.handoff_in.allows(Initiator::Automatic)),
        ("Automatic outgoing handoff", caps.handoff_out.allows(Initiator::Automatic)),
        ("Run as a subagent", caps.subagent),
        ("Start subagents", caps.spawns_subagents),
        ("Mid-turn steering", if adapter.steers { Capability::Allowed } else { Capability::Refused("This adapter queues follow-up messages until the turn finishes.") }),
        ("Usage readings", if adapter.usage.is_some() { Capability::Allowed } else { Capability::Refused("This adapter does not provide usage readings.") }),
        ("Client control of native compaction", if adapter.native_compaction_managed { Capability::Allowed } else { Capability::Refused("Native compaction is not controlled through this adapter.") }),
    ];
    declared.into_iter().map(|(label, capability)| (label, capability.allowed(),
        if capability.allowed() { "Supported" } else { "Unavailable" }, capability.refusal())).collect()
}

/// Adds `item` to `items`, or removes it when it is there.
pub fn flip<T: PartialEq>(items: &mut Vec<T>, item: T) {
    match items.iter().position(|i| *i == item) {
        Some(at) => {
            items.remove(at);
        }
        None => items.push(item),
    }
}

/// Flips whether `harness` is enabled as a provider in m4code. Ensures at least one harness stays enabled.
fn flip_harness_enabled(ws: &mut Workspace, harness: Harness, cx: &mut Context<Workspace>) {
    let enabled = {
        let Some(Modal::Settings { subagents, default_model, .. }) = ws.modal.as_mut() else { return };
        if subagents.enabled.contains(&harness) {
            if subagents.enabled.len() <= 1 {
                return;
            }
            subagents.enabled.retain(|&h| h != harness);
            if default_model.harness == harness
                && let Some(&next) = subagents.enabled.first()
            {
                let all = ws.store.data.settings.aggregated_models_for(&subagents.enabled);
                *default_model = crate::store::Settings::resolve_model(&all, next, None);
            }
            false
        } else {
            subagents.enabled.push(harness);
            true
        }
    };
    if enabled {
        ws.load_models(cx);
    }
}

/// One switch controls receiving delegated work and delegation across harnesses.
fn flip_cross_harness(ws: &mut Workspace, harness: Harness, cx: &mut Context<Workspace>) {
    let Some(Modal::Settings { subagents, .. }) = ws.modal.as_mut() else { return };
    let mut access = subagents.access(harness);
    access.cross_harness = !access.cross_harness;
    access.on = access.cross_harness;
    subagents.set_access(harness, access);
    ws.load_models(cx);
}

fn harness_status(harness: Harness, enabled: bool, subagents: &crate::app::HarnessesForm, lookup: &Lookup, model_count: usize) -> (String, gpui::Hsla) {
    if !enabled {
        return ("Disabled".into(), theme().subtle_foreground);
    }
    if lookup.loading && model_count == 0 {
        return (format!("Enabled · loading {} models…", harness.name()), theme().muted_foreground);
    }
    if let Some(why) = &lookup.error
        && model_count == 0
    {
        return (format!("Enabled · couldn't load models ({why})"), theme().warning);
    }
    let access = subagents.access(harness);
    let count = if subagents.picked(harness).is_empty() { model_count } else { subagents.picked(harness).len() };
    let models = match count {
        0 => String::new(),
        1 => " · 1 model".to_string(),
        n => format!(" · {n} models"),
    };
    let delegation = if access.on { "cross-harness subagents on" } else { "cross-harness subagents off" };
    (format!("Enabled{models} · {delegation}"), theme().muted_foreground)
}

/// The models to list: those the agent offers, then any switched on that it no longer offers, so they can be
/// switched off. The flag says whether the agent still offers the model.
pub(super) fn model_rows(catalog: &[AgentModel], on: &[String]) -> Vec<(AgentModel, bool)> {
    let gone = on.iter().filter(|id| !catalog.iter().any(|m| &m.id == *id)).map(|id| AgentModel { id: id.clone(), name: id.clone() });
    catalog.iter().cloned().map(|m| (m, true)).chain(gone.map(|m| (m, false))).collect()
}

/// How many of the models the agent offers are switched on, as "2 of 107 on". A model switched on that the agent no
/// longer offers isn't one of them.
pub(super) fn on_count(catalog: &[AgentModel], on: &[String]) -> String {
    format!("{} of {} on", catalog.iter().filter(|m| is_ticked(on, &m.id)).count(), catalog.len())
}

/// Whether `id` is ticked. A harness nobody has set up yet has every model ticked, which `ticked` saves as no list.
pub(super) fn is_ticked(ticked: &[String], id: &str) -> bool {
    ticked.is_empty() || ticked.iter().any(|t| t == id)
}

/// Tick or untick `id` among `catalog`'s models. The last ticked model stays ticked, so the model picker always has
/// one and an empty list only ever means all of them.
pub(super) fn tick(ticked: &mut Vec<String>, catalog: &[AgentModel], id: &str) {
    if ticked.is_empty() {
        *ticked = catalog.iter().map(|m| m.id.clone()).collect();
    }
    match ticked.iter().position(|t| t == id) {
        Some(_) if ticked.len() == 1 => {}
        Some(at) => {
            ticked.remove(at);
        }
        None => ticked.push(id.to_string()),
    }
}

/// Whether `model` fits what was typed in the picker's search: every word, in its name or its id, ignoring case.
pub(super) fn model_matches(query: &str, model: &AgentModel) -> bool {
    let haystack = format!("{} {}", model.name, model.id).to_lowercase();
    query.to_lowercase().split_whitespace().all(|word| haystack.contains(word))
}

impl Workspace {
    /// The Harnesses page: enable Claude Code, Codex, OpenCode and Antigravity, and open each provider's configuration
    /// popup with model, subagent and provider-specific controls.
    pub(crate) fn harnesses_page(&self, cx: &mut Context<Self>) -> Div {
        let Some(Modal::Settings { subagents, .. }) = &self.modal else { return div() };
        let mut rows: Vec<AnyElement> = Vec::new();
        for harness in Harness::registered() {
            let enabled = subagents.enabled.contains(&harness);
            let agent = harness.agent();
            let lookup = self.model_lookups.get(harness);
            let model_count = self.store.data.settings.harness_models(harness).len();
            let (status, tone) = harness_status(harness, enabled, subagents, lookup, model_count);

            let mut about = div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap(px(3.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(logos::draw(logos::for_harness(harness), 15.0, if enabled { theme().primary } else { theme().subtle_foreground }))
                        .child(div().text_size(crate::panels::scale::px(13.0)).font_weight(FontWeight::MEDIUM).text_color(if enabled { theme().foreground } else { theme().muted_foreground }).child(harness.name())),
                )
                .child(div().text_size(crate::panels::scale::px(11.5)).text_color(tone).child(status));
            let access = subagents.access(harness);
            let picked = subagents.picked(harness);
            let catalog = self.store.data.settings.harness_models(harness);
            if enabled && access.on && !picked.is_empty() {
                let names: Vec<&str> = picked.iter().map(|id| catalog.iter().find(|m| &m.id == id).map_or(id.as_str(), |m| m.name.as_str())).collect();
                about = about.child(faint(names.join(" · ")).overflow_hidden().text_ellipsis().whitespace_nowrap());
            }
            if let Some(warning) = agent.and_then(|agent| agent.provider().warning).filter(|_| access.on) {
                about = about.child(warn(warning));
            }
            let key = harness.name().to_lowercase().replace(' ', "-");
            let open = div()
                .id(SharedString::from(format!("settings-harness-{key}-open")))
                .flex()
                .flex_1()
                .min_w_0()
                .items_center()
                .gap(px(12.0))
                .cursor_pointer()
                .child(about)
                .child(
                    btn::button(format!("settings-harness-{key}-configure"), None, "Configure", btn::Variant::Outline, btn::Size::Xs)
                        .flex_none()
                        .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                            cx.stop_propagation();
                            ws.open_harness_dialog(harness, window, cx);
                            cx.notify();
                        })),
                )
                .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                    ws.open_harness_dialog(harness, window, cx);
                    cx.notify();
                }));
            let toggle = btn::focusable(SharedString::from(format!("settings-harness-{key}")))
                .accessibility_label(format!("{} {}", if enabled { "Disable" } else { "Enable" }, harness.name()))
                .pl(px(12.0))
                .flex()
                .items_center()
                .cursor_pointer()
                .child(switch(enabled))
                .on_click(cx.listener(move |ws, _: &ClickEvent, _w, cx| {
                    flip_harness_enabled(ws, harness, cx);
                    cx.notify();
                }));
            rows.push(
                div()
                    .flex()
                    .items_center()
                    .w_full()
                    .px(px(14.0))
                    .py(px(12.0))
                    .hover(|s| s.bg(theme().accent))
                    .child(open)
                    .child(toggle)
                    .into_any_element(),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(settings_group("CONNECTED HARNESSES", None, rows))
            .child(faint("Click any harness to choose its models and configure how it behaves.").px(px(14.0)))
    }

    fn subagent_controls(&self, harness: Harness, cx: &mut Context<Self>) -> Div {
        let Some(Modal::Settings { subagents, .. }) = &self.modal else { return div() };
        let access = subagents.access(harness);
        let key = harness.key();
        let refusal = harness.adapter().capabilities.subagent.refusal();
        let mut controls = div().flex().flex_col().child(switch_row(
            SharedString::from(format!("agent-dialog-{key}-cross-harness")),
            "Allow cross-harness subagents",
            Some(refusal.unwrap_or("Let other harnesses call this one, and let this harness call others").into()),
            access.cross_harness && refusal.is_none(),
            move |ws, _, cx| flip_cross_harness(ws, harness, cx),
            cx,
        ).disabled(refusal.is_some()));
        if let Some(warning) = harness.agent().and_then(|agent| agent.provider().warning) {
            controls = controls.child(div().px(px(14.0)).pb(px(8.0)).child(warn(warning)));
        }
        controls
    }

    /// Harness details use the same scroll pane and section layout as every Settings page.
    pub(crate) fn render_harness_settings(&self, _window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let Some(Modal::Settings { claude, skills_field, costly, subagents, chrome, .. }) = &self.modal else { return None };
        let picking = subagents.picking.as_ref()?;
        let harness = picking.harness;
        let enabled = subagents.enabled.contains(&harness);
        let mut connection = vec![switch_row(
            "harness-detail-enabled",
            format!("Enable {}", harness.name()),
            Some("Make this harness available for new sessions".into()),
            enabled,
            move |ws, _, cx| flip_harness_enabled(ws, harness, cx),
            cx,
        ).into_any_element()];
        if harness == Harness::Claude {
            connection.push(div().px(px(14.0)).py(px(10.0)).flex().flex_col().gap(px(6.0))
                .child(section_label("EXECUTABLE"))
                .child(executable_field(claude, self)).into_any_element());
        }
        let mut body = div().flex().flex_col().gap(px(20.0)).w_full()
            .child(settings_group("CONNECTION", None, connection))
            .child(settings_group("DELEGATION", None, vec![self.subagent_controls(harness, cx).into_any_element()]));
        let catalog = self.store.data.settings.harness_models(harness);
        let count = on_count(catalog, subagents.picked(harness));
        let open = matches!(self.picker_kind(), Some(WorkspacePicker::HarnessModels { .. }));
        let about = if subagents.access(harness).on { "Shown in the model picker and offered to subagents" } else { "Shown in the model picker" };
        let lookup = self.model_lookups.get(harness);
        let current = if !catalog.is_empty() { count }
            else if lookup.loading { "Loading…".into() }
            else if lookup.error.is_some() { "Couldn't load".into() }
            else { "None loaded".into() };
        let selector = super::settings_view::settings_picker_row("harness-model-selector", ("Models", about), current, open,
            move |ws, at, window, cx| ws.open_harness_models_picker(harness, at, window, cx), cx);
        let mut models = vec![selector.into_any_element()];
        if let Some(why) = lookup.error.as_ref().filter(|_| catalog.is_empty()) {
            models.push(div().px(px(14.0)).pb(px(8.0)).child(warn(format!("Couldn't load {}'s models: {why}", harness.name()))).into_any_element());
        }
        body = body.child(settings_group("MODELS", None, models));
        if harness == Harness::Claude {
            let mut options = vec![switch_row("harness-dialog-claude-chrome", "Claude in Chrome",
                Some("Start new sessions with browser integration".into()), *chrome,
                |ws, _, _| { if let Some(Modal::Settings { chrome, .. }) = &mut ws.modal { *chrome = !*chrome; } }, cx).into_any_element()];
            options.push(div().px(px(14.0)).py(px(10.0)).flex().flex_col().gap(px(6.0))
                .child(div().text_size(crate::panels::scale::px(13.0)).child("Hidden skills"))
                .child(faint("Comma-separated names. Claude Code’s settings.json takes precedence."))
                .child(Input::new(skills_field)).into_any_element());
            for found in costly { options.push(div().px(px(14.0)).child(warn(format!("{} is set in {}. {}", found.name, found.source, found.effect))).into_any_element()); }
            body = body.child(settings_group("CLAUDE CODE", None, options));
        }
        body = body.child(self.adapter_features(harness));
        Some(body.into_any_element())
    }

    fn adapter_features(&self, harness: Harness) -> Div {
        let rows = adapter_feature_rows(harness).into_iter().map(|(label, available, status, reason)| {
            div().px(px(14.0)).py(px(7.0)).flex().items_start().gap(px(10.0))
                .child(icon(if available { IconName::Check } else { IconName::X }, 13.0)
                    .text_color(if available { theme().muted_foreground } else { theme().subtle_foreground }))
                .child(div().flex_1().min_w_0().flex().flex_col().gap(px(3.0))
                    .child(div().text_size(crate::panels::scale::px(12.0)).child(label))
                    .children(reason.map(faint)))
                .child(faint(status)).into_any_element()
        }).collect();
        div().flex().flex_col().gap(px(8.0))
            .child(settings_group("ADAPTER FEATURES", None, rows))
            .child(div().px(px(14.0)).child(faint("Reported by the adapter. Availability can also depend on session commands, authentication and your settings.")))
    }

    /// The Projects section on the Projects page: every project in the sidebar's order, to rename, move, remove, or
    /// add to. Changes apply at once; a removal can be undone from its toast.
    pub(crate) fn projects_section(&self, renaming: Option<&(crate::store::Id, gpui::Entity<InputState>)>, cx: &mut Context<Self>) -> Div {
        let projects = &self.store.data.projects;
        let add = btn::text("project-add", "Add project…", false)
            .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| ws.act(Act::ImportProject, window, cx)))
            .into_any_element();
        if projects.is_empty() {
            let empty = div()
                .px(px(14.0))
                .py(px(14.0))
                .text_size(crate::panels::scale::px(12.0))
                .text_color(theme().subtle_foreground)
                .child("No projects yet. Add a folder to group sessions and switch branches.")
                .into_any_element();
            return settings_group("PROJECTS", Some(add), vec![empty]);
        }
        let pressed = |b: gpui_kit::base::Button, act: Act, cx: &mut Context<Self>| b.on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| ws.act(act.clone(), window, cx)));
        let last = projects.len() - 1;
        let mut rows: Vec<AnyElement> = Vec::with_capacity(projects.len());
        for (i, p) in projects.iter().enumerate() {
            let id = p.id.clone();
            let editing = renaming.filter(|(r, _)| *r == p.id);
            let name = match editing {
                Some((_, input)) => Input::new(input).into_any_element(),
                None => div().text_size(crate::panels::scale::px(13.0)).font_weight(FontWeight::MEDIUM).child(p.name.clone()).into_any_element(),
            };
            let mut buttons = div().flex().flex_none().gap(px(5.0));
            buttons = if editing.is_some() {
                buttons
                    .child(pressed(btn::text(format!("project-cancel-{i}"), "Cancel", false), Act::CancelProjectRename, cx))
                    .child(pressed(btn::text(format!("project-save-{i}"), "Save", true), Act::SaveProjectName, cx))
            } else {
                buttons.child(pressed(btn::text(format!("project-rename-{i}"), "Rename", false), Act::RenameProject(id.clone()), cx))
            };
            buttons = buttons
                .child(if i == 0 { btn::disabled(format!("project-up-{i}"), "Up") } else { pressed(btn::text(format!("project-up-{i}"), "Up", false), Act::MoveProject(id.clone(), true), cx) })
                .child(if i == last { btn::disabled(format!("project-down-{i}"), "Down") } else { pressed(btn::text(format!("project-down-{i}"), "Down", false), Act::MoveProject(id.clone(), false), cx) })
                .child(pressed(btn::text(format!("project-remove-{i}"), "Remove", false), Act::RemoveProject(id), cx));
            rows.push(
                div()
                    .flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .gap(px(12.0))
                    .px(px(14.0))
                    .py(px(10.0))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .gap(px(2.0))
                            .child(name)
                            .child(faint(p.path.to_string_lossy().into_owned()).overflow_hidden().text_ellipsis().whitespace_nowrap()),
                    )
                    .child(buttons)
                    .into_any_element(),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(settings_group("PROJECTS", Some(add), rows))
            .child(faint("Removing a project keeps its sessions; they show under no project."))
    }
}

/// Executable fields share Setup's validation and show every invalid value beside its input.
pub fn executable_field(input: &gpui::Entity<InputState>, ws: &Workspace) -> Div {
    let error = ws.executable_errors.get(&input.entity_id()).cloned();
    div().flex().flex_col().gap(px(4.0)).child(Input::new(input).h(px(28.0)).text_size(crate::panels::scale::px(12.0)))
        .children(error.map(|message| div().text_size(crate::panels::scale::px(11.0)).text_color(theme().destructive).child(message)))
}

#[cfg(test)]
mod tests {
    #[test]
    fn feature_checklist_uses_adapter_capabilities_and_keeps_user_only_handoffs_distinct() {
        for harness in Harness::registered() {
            let rows = adapter_feature_rows(harness);
            let caps = harness.adapter().capabilities;
            assert_eq!(rows.iter().find(|r| r.0 == "Rewind messages").unwrap().1, caps.rewind.allowed());
            assert_eq!(rows.iter().find(|r| r.0 == "Background text generation").unwrap().1, caps.text_generation.allowed());
        }
        #[cfg(feature = "adapter-antigravity")]
        {
            let rows = adapter_feature_rows(Harness::Antigravity);
            assert!(rows.iter().find(|r| r.0 == "Hand off to another harness").unwrap().1);
            assert!(!rows.iter().find(|r| r.0 == "Automatic outgoing handoff").unwrap().1);
        }
    }

    #[test]
    fn harness_subtitles_use_the_same_subagent_status_for_every_provider() {
        let form = crate::app::HarnessesForm {
            enabled: Harness::ALL.to_vec(),
            agents: crate::harness::subagents::AgentMap::from_fn(|_| crate::app::AgentForm { on: true, edits: true, models: vec![], cross_harness: true }),
            claude_models: vec![], claude_subagents: Default::default(), picking: None,
        };
        for harness in Harness::registered() {
            assert_eq!(harness_status(harness, true, &form, &Lookup::default(), 2).0, "Enabled · 2 models · cross-harness subagents on");
        }
    }
    #[test]
    fn every_model_starts_ticked_and_the_last_one_stays() {
        use super::{is_ticked, tick};
        let catalog: Vec<_> = ["a", "b", "c"].iter().map(|id| crate::harness::subagents::AgentModel { id: id.to_string(), name: id.to_string() }).collect();
        let mut ticked = vec![];
        assert!(catalog.iter().all(|m| is_ticked(&ticked, &m.id)), "a harness nobody set up has every model ticked");
        tick(&mut ticked, &catalog, "b");
        assert_eq!(ticked, ["a", "c"]);
        tick(&mut ticked, &catalog, "a");
        tick(&mut ticked, &catalog, "c");
        assert_eq!(ticked, ["c"], "the last ticked model can't be unticked");
        tick(&mut ticked, &catalog, "b");
        assert_eq!(ticked, ["c", "b"]);
    }

    use super::*;

    fn model(id: &str, name: &str) -> AgentModel {
        AgentModel { id: id.into(), name: name.into() }
    }

    #[test]
    fn every_word_of_the_search_must_match_the_name_or_the_id() {
        let sol = model("openai/gpt-6.1-sol", "openai/GPT-6.1 Sol");
        assert!(model_matches("", &sol));
        assert!(model_matches("  GPT sol ", &sol));
        assert!(model_matches("openai 6.1", &sol));
        assert!(!model_matches("gpt luna", &sol));
        assert!(!model_matches("anthropic", &sol));
    }

    #[test]
    fn the_count_leaves_out_models_no_longer_offered() {
        let catalog = [model("a", "A"), model("b", "B")];
        let on = ["a".to_string(), "gone-1".into(), "gone-2".into()];
        assert_eq!(on_count(&catalog, &on), "1 of 2 on");
    }

    #[test]
    fn models_no_longer_offered_come_last_and_marked() {
        let rows = model_rows(&[model("a", "A")], &["gone".to_string(), "a".into()]);
        assert_eq!(rows, vec![(model("a", "A"), true), (model("gone", "gone"), false)]);
    }
}
