//! Settings: where notebooks live (this Mac or a server), models, API keys, podcasts and looks.

use notebook_engine::providers::ProviderKind;
use notebook_engine::settings::{self as engine_settings, Mode, SyncError};
use rune::{El, ScrollSpec, Tx, col, el, row, text};
use rune_kit::{Button, ButtonVariant, Combobox, ComboboxOption, ControlSize, IconButton, IconButtonVariant, Spinner, Switch, TextInput};

use super::icons;
use super::select::Select;
use crate::state::{App, Check, ModelList, SettingsForm, models_for};
use crate::theme::{self, Look};

fn form(s: &mut App) -> &mut SettingsForm {
    s.form.as_mut().expect("settings are open")
}

/// What each provider's key is good for.
fn key_hint(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Anthropic => "Questions, figures and insights. Add OpenAI or Gemini for search.",
        ProviderKind::OpenAi | ProviderKind::Gemini => "Questions, figures, search and transcription.",
        ProviderKind::OpenAiCompatible => "OpenRouter or a local model server. Local servers may not need a key.",
    }
}

fn role_label(role: &str) -> &'static str {
    match role {
        "chat" => "Questions and figures",
        "transformation" => "Insights and study decks",
        "embedding" => "Search",
        _ => "Audio and video",
    }
}

fn role_help(role: &str) -> Option<&'static str> {
    match role {
        "embedding" => Some("Finds related passages by meaning. Without one, search matches words."),
        "transformation" => Some("Writes insights and study decks. Leave empty to use the chat model."),
        "transcription" => Some("Turns audio and video sources into text. Up to 25 MB with OpenAI, 19 MB with Gemini."),
        _ => None,
    }
}

pub fn view(s: &App, l: &Look, layer: &mut Vec<El>) -> El {
    use crate::configuration::SettingsTab;
    let k = &l.k;
    let Some(f) = &s.form else { return col() };
    let tabs = if l.phone() { ["Connect", "Keys", "Models", "General"] } else { SettingsTab::ALL.map(SettingsTab::label) };
    let navigation = super::pages::choices(&tabs, SettingsTab::ALL.iter().position(|t| *t == f.tab), "Settings sections", l, |s: &mut App, i, _: &mut Tx| {
        form(s).tab = SettingsTab::ALL[i]
    })
    .key("settings-tabs");
    let active = if s.settings.mode == Mode::Local { format!("Notebooks on {}", theme::THIS_DEVICE) } else { format!("Server: {}", s.settings.server_url) };
    let header = col()
        .w_full()
        .max_w(760.0)
        .px(l.pad())
        .pt(l.pad_top())
        .pb(18.0)
        .gap(10.0)
        .child(text("Settings").font(theme::READ_TITLE).color(l.text))
        .child(text(active).font(k.ty.small).color(l.text2).wrap())
        .child(navigation);
    let mut body = col().w_full().max_w(760.0).px(l.pad()).pt(20.0).pb(40.0 + l.inset_bottom).gap(24.0);
    let server_managed = s.settings.mode == Mode::Remote && matches!(f.tab, SettingsTab::Keys | SettingsTab::Models);
    if server_managed {
        body = body.child(
            section("Managed by your server", l)
                .child(
                    text("Your server supplies the models and provider keys for its notebooks. Manage those settings on the Server page.")
                        .font(k.ty.ui)
                        .color(l.text2)
                        .wrap(),
                )
                .child(
                    row().child(
                        Button::new("Open server settings")
                            .variant(ButtonVariant::Contrast)
                            .on_press(|s: &mut App| s.go(crate::state::Route::Server))
                            .build(k)
                            .min_h(44.0)
                            .key("open-server-settings"),
                    ),
                ),
        );
    }
    match f.tab {
        SettingsTab::Keys | SettingsTab::Models if server_managed => {}
        SettingsTab::Connection => {
            let mode = super::pages::choices(
                &[if cfg!(target_os = "ios") { "On this iPhone" } else { "On this Mac" }, "Server"],
                Some(if f.mode == Mode::Local { 0 } else { 1 }),
                "Where notebooks live",
                l,
                |s: &mut App, i, _: &mut Tx| {
                    let f = form(s);
                    f.connection_edited();
                    f.mode = if i == 0 { Mode::Local } else { Mode::Remote };
                },
            )
            .key("mode");
            body = body.child(
                section("Where notebooks live", l)
                    .child(text("Use notebooks stored on this device or connect to your own Open Notebook server.").font(k.ty.ui).color(l.text2).wrap())
                    .child(mode),
            );
            if f.mode == Mode::Local {
                body = body.child(
                    section("On-device notebooks", l)
                        .child(
                            text("Materials and conversations are stored here. Questions go to the provider you choose, using your saved key.")
                                .font(k.ty.ui)
                                .color(l.text2)
                                .wrap(),
                        )
                        .child(if s.settings.mode == Mode::Local {
                            text("On-device mode is active").font(k.ty.ui_medium).color(l.ok)
                        } else {
                            Button::new(if cfg!(target_os = "ios") { "Use this iPhone" } else { "Use this Mac" })
                                .variant(ButtonVariant::Contrast)
                                .on_press(App::use_local_mode)
                                .build(k)
                                .min_h(44.0)
                                .key("use-local")
                        })
                        .child(
                            row()
                                .w_full()
                                .gap(8.0)
                                .flex_wrap()
                                .child(
                                    Button::new("Manage API keys")
                                        .on_press(|s: &mut App| form(s).tab = SettingsTab::Keys)
                                        .build(k)
                                        .min_h(44.0)
                                        .key("settings-open-keys"),
                                )
                                .child(
                                    Button::new("Choose models")
                                        .on_press(|s: &mut App| form(s).tab = SettingsTab::Models)
                                        .build(k)
                                        .min_h(44.0)
                                        .key("settings-open-models"),
                                ),
                        ),
                );
                body = body.child(save_feedback(f, l));
            } else {
                body = body.child(server_section(s, f, l));
            }
        }
        SettingsTab::Keys => {
            let saved = f.saved_keys.iter().filter(|n| n.starts_with("provider:")).count();
            let busy = f.checks.values().any(|c| *c == Check::Checking);
            body = body.child(
                section("Provider access", l)
                    .child(
                        text(format!(
                            "{saved} {} saved. Keys stay hidden; leave a field empty to keep its saved key.",
                            if saved == 1 { "key" } else { "keys" }
                        ))
                        .font(k.ty.ui)
                        .color(l.text2)
                        .wrap(),
                    )
                    .child(
                        text("Checks confirm access to the provider’s model list. Availability and billing can vary by model.")
                            .font(k.ty.small)
                            .color(l.text3)
                            .wrap(),
                    )
                    .child(
                        row().child(
                            Button::new(if busy { "Checking keys…" } else { "Check saved keys" })
                                .disabled(saved == 0 || busy)
                                .on_press(App::check_saved_keys)
                                .build(k)
                                .min_h(44.0)
                                .key("check-saved-keys"),
                        ),
                    ),
            );
            for (i, (kind, edit)) in f.keys.iter().enumerate() {
                body = body.child(provider_key(s, f, i, *kind, edit, l));
            }
            body = body.child(elevenlabs_key(f, l));
        }
        SettingsTab::Models => {
            let busy = f.recommendation == Some(Check::Checking);
            let mut setup = col().w_full().padding(20.0).gap(12.0).radius(8.0).fill(l.raised)
                .child(text("Recommended setup").font(theme::READ_HEADING).color(l.text))
                .child(text("Choose balanced models for questions, figures, insights, search and transcription from providers with saved keys. This replaces and saves supported selections.").font(k.ty.ui).color(l.text2).wrap())
                .child(row().w_full().gap(10.0).center_y().flex_wrap()
                    .child(Button::new(if busy { "Choosing models…" } else { "Recommended models" }).variant(ButtonVariant::Contrast).disabled(busy).on_press(App::recommend_models).build(k).min_h(44.0).key("recommend-models"))
                    .child(Button::new("Manage API keys").variant(ButtonVariant::Ghost).on_press(|s: &mut App| form(s).tab = SettingsTab::Keys).build(k).min_h(44.0)));
            if let Some(check) = &f.recommendation {
                setup = setup.child(operation_feedback(check, "Checking available models with your providers…", l).key("recommendation-feedback"));
            }
            body = body.child(setup);
            let mut models = section("Models for each task", l).child(
                text("Choose a provider and model, or type a model name. Figure questions need a model that accepts images.")
                    .font(k.ty.small)
                    .color(l.text3)
                    .wrap(),
            );
            for role in crate::state::MODEL_ROLES {
                models = models.child(model_row(f, role, l, layer));
            }
            body = body
                .child(models)
                .child(
                    row().child(
                        Button::new("Save changes")
                            .variant(ButtonVariant::Contrast)
                            .disabled(busy)
                            .on_press(App::save_settings)
                            .build(k)
                            .min_h(44.0)
                            .key("save-settings"),
                    ),
                )
                .child(save_feedback(f, l))
                .child(super::notebook::rule(l))
                .child(
                    section("Search index", l)
                        .child(
                            text("Changing an embedding model affects new materials. Rebuild to bring existing materials onto the same model.")
                                .font(k.ty.small)
                                .color(l.text3)
                                .wrap(),
                        )
                        .child(rebuild_row(f, l)),
                );
        }
        SettingsTab::Preferences => {
            let appearance = ["system", "light", "dark"];
            body = body
                .child(
                    section("Appearance", l).child(
                        super::pages::choices(
                            &["System", "Light", "Dark"],
                            appearance.iter().position(|a| *a == s.settings.appearance),
                            "Appearance",
                            l,
                            move |s: &mut App, i, _: &mut Tx| {
                                s.settings.appearance = appearance[i].into();
                                s.save_settings();
                            },
                        )
                        .key("appearance"),
                    ),
                )
                .child(save_feedback(f, l))
                .child(
                    section("Key storage", l)
                        .child(text("Keys are stored privately on this device and sent only to their provider.").font(k.ty.small).color(l.text3).wrap())
                        .child(sync_row(s, f, l)),
                )
                .child(podcasts(l));
        }
    }
    col().key("settings").w_full().h_full().min_h(0.0).items_center().child(header).child(super::notebook::rule(l)).child(
        col()
            .key(format!("settings-scroll-{}", f.tab.label()))
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .items_center()
            .child(body)
            .scroll(ScrollSpec { stick_bottom: false }),
    )
}

fn operation_feedback(check: &Check, progress: &str, l: &Look) -> El {
    match check {
        Check::Checking => row()
            .w_full()
            .gap(8.0)
            .center_y()
            .child(Spinner::new().size(16.0).build(&l.k))
            .child(text(progress.to_owned()).font(l.k.ty.small).color(l.text2).wrap().min_w(0.0).grow(1.0)),
        Check::Working(message) => text(message.clone()).font(l.k.ty.small_medium).color(l.ok).wrap(),
        Check::Failed(error) => text(error.clone()).font(l.k.ty.small).color(l.danger).wrap(),
    }
}

fn save_feedback(f: &SettingsForm, l: &Look) -> El {
    f.save.as_ref().map(|c| operation_feedback(c, "Saving changes…", l).key("settings-save-feedback")).unwrap_or_else(el)
}

fn section(title: &str, l: &Look) -> El {
    col().w_full().gap(if l.phone() { 16.0 } else { 10.0 }).child(text(title.to_owned()).font(l.k.ty.ui_medium).color(l.text))
}

fn field_row(label: &str, field: El, l: &Look) -> El {
    col().w_full().gap(4.0).child(text(label.to_owned()).font(l.k.ty.small).color(l.text2)).child(field)
}

/// A check's status: a spinner, "Working, …" or "Not working".
fn status(check: Option<&Check>, saved: bool, l: &Look) -> El {
    match check {
        Some(Check::Checking) => {
            row().gap(6.0).center_y().child(Spinner::new().size(14.0).build(&l.k)).child(text("Checking…").font(l.k.ty.small).color(l.text2))
        }
        Some(Check::Working(message)) => text(message.clone()).font(l.k.ty.small_medium).color(l.ok).wrap(),
        Some(Check::Failed(_)) => text(if saved { "Saved · check failed" } else { "Connection failed" }).font(l.k.ty.small_medium).color(l.danger),
        None => text(if saved { "Saved · not checked" } else { "Not added" }).font(l.k.ty.small).color(l.text3),
    }
}

fn error_of(check: Option<&Check>) -> Option<&str> {
    match check {
        Some(Check::Failed(e)) => Some(e),
        _ => None,
    }
}

fn podcasts(l: &Look) -> El {
    let k = &l.k;
    section("Podcasts", l)
        .child(text("Episode profiles set the angle and length; speaker profiles set who talks and with which voices.").font(k.ty.small).color(l.text3).wrap())
        .child(
            row().child(
                Button::new("Podcast profiles")
                    .on_press(|s: &mut App| s.open_profiles())
                    .build(k)
                    .min_h(if l.size != theme::Size::Desk { 44.0 } else { 0.0 })
                    .min_w(if l.size != theme::Size::Desk { 44.0 } else { 0.0 })
                    .key("open-profiles"),
            ),
        )
}

fn rebuild_row(f: &SettingsForm, l: &Look) -> El {
    let k = &l.k;
    let busy = f.rebuild == Some(Check::Checking);
    let mut r = row().w_full().gap(10.0).center_y().child(
        Button::new("Rebuild search index")
            .icon(&icons::RETRY)
            .disabled(busy)
            .on_press(|s: &mut App| s.ask_to("Rebuild", "Read and index every source again with the current embedding model?", |s| s.rebuild_local_index()))
            .build(k)
            .min_h(if l.size != theme::Size::Desk { 44.0 } else { 0.0 })
            .min_w(if l.size != theme::Size::Desk { 44.0 } else { 0.0 })
            .key("rebuild-index"),
    );
    match &f.rebuild {
        Some(Check::Checking) => r = r.child(Spinner::new().size(14.0).build(k)),
        Some(Check::Working(what)) => r = r.child(text(what.clone()).font(k.ty.small).color(l.text2)),
        Some(Check::Failed(e)) => r = r.child(text(e.clone()).font(k.ty.small).color(l.danger).shrink().min_w(0.0).wrap()),
        None => {}
    }
    r
}

fn model_row(f: &SettingsForm, role: &'static str, l: &Look, layer: &mut Vec<El>) -> El {
    let k = &l.k;
    let label = role_label(role);
    let providers = f.providers(role);
    let mut kind = f.provider_of(role);
    let current = match role {
        "chat" => &f.chat_model,
        "transformation" => &f.transformation_model,
        "embedding" => &f.embedding_model,
        _ => &f.transcription_model,
    }
    .text
    .trim()
    .to_owned();
    // A saved choice stays visible even after its key is removed.
    let mut choices = providers.clone();
    if !current.is_empty() && !choices.contains(&kind) {
        choices.push(kind);
    }
    if choices.is_empty() {
        return field_row(label, text("Add an API key on the API keys tab first").font(k.ty.small).color(l.text3), l);
    }
    if !choices.contains(&kind) {
        kind = choices[0];
    }
    let names: Vec<&str> = choices.iter().map(|p| p.label()).collect();
    let select = Select::new(format!("{role}-provider"), &names)
        .label(format!("{label} provider"))
        .current(Some(kind.label()))
        .width(if l.phone() { l.width - 2.0 * l.pad() } else { 190.0 })
        .open(f.open_select == Some(role))
        .on_toggle(move |s: &mut App| {
            let f = form(s);
            f.open_select = if f.open_select == Some(role) { None } else { Some(role) };
        })
        .on_close(|s: &mut App| form(s).open_select = None)
        .on_choose(move |s: &mut App, name: &str, _: &mut Tx| {
            let f = form(s);
            f.open_select = None;
            if let Some(p) = ProviderKind::ALL.into_iter().find(|p| p.label() == name) {
                f.edited();
                f.choose_provider(role, p);
                if !matches!(f.models.get(&p), Some(ModelList::Ready(_))) {
                    s.load_models(p, false);
                }
                // A pick is saved at once; leaving the page can't lose it.
                s.save_settings();
            }
        })
        .build(k, layer)
        .min_h(if l.size != theme::Size::Desk { 44.0 } else { 0.0 });
    let state = f.pickers.get(role).cloned().unwrap_or_default();
    let list = f.models.get(&kind);
    let mut options: Vec<ComboboxOption> = match list {
        Some(ModelList::Ready(models)) => models_for(role, kind, models)
            .into_iter()
            .map(|m| {
                let mut o = ComboboxOption::new(m.id.clone(), m.id.clone());
                o.detail = m.label.clone().unwrap_or_default();
                o
            })
            .collect(),
        _ => Vec::new(),
    };
    if !current.is_empty() && !options.iter().any(|o| o.id == current) {
        options.insert(0, ComboboxOption::new(current.clone(), current.clone()));
    }
    let typed = state.search.text.trim().to_owned();
    if !typed.is_empty() && !options.iter().any(|o| o.id == typed) {
        options.insert(0, ComboboxOption::new(typed.clone(), format!("Use {typed}")));
    }
    let picker = if l.phone() {
        model_sheet(role, label, &current, &state, &options, l, layer)
    } else {
        Combobox::new(format!("{role}-model"), &state, &options, move |s: &mut App| form(s).pickers.entry(role).or_default())
            .label(label)
            .current((!current.is_empty()).then_some(current.as_str()))
            .placeholder(if list == Some(&ModelList::Loading) { "Loading models" } else { "Choose or type a model" })
            .search_placeholder("Search or type a model name")
            .empty_message("No models; type a name")
            .width(if l.phone() { (l.width - 2.0 * l.pad()).max(200.0) } else { 320.0 })
            .on_choose(move |s: &mut App, id: &str, _: &mut Tx| {
                form(s).edited();
                *form(s).slot(role).1 = rune::TextEdit::new(id);
                s.save_settings();
            })
            .build(k, layer)
            .min_h(if l.size != theme::Size::Desk { 44.0 } else { 0.0 })
    };
    // Provider beside the model on a wide window, above it on a phone.
    let pair = if l.phone() {
        col().w_full().gap(8.0).child(select).child(picker)
    } else {
        row().w_full().gap(8.0).center_y().child(select).child(col().grow(1.0).min_w(0.0).child(picker))
    };
    let mut c = field_row(label, pair, l);
    if let Some(ModelList::Failed(_)) = list {
        c = c.child(text("Couldn’t load models. Check this provider on the API keys tab, or type a model name.").font(k.ty.small).color(l.danger).wrap());
    } else if let Some(help) = role_help(role) {
        c = c.child(text(help).font(k.ty.small).color(l.text3).wrap());
    }
    c
}

fn model_sheet(
    role: &'static str,
    label: &str,
    current: &str,
    state: &rune_kit::ComboboxState,
    options: &[ComboboxOption],
    l: &Look,
    layer: &mut Vec<El>,
) -> El {
    if state.open {
        let mut list = col().w_full().gap(4.0).child(
            TextInput::new(&state.search)
                .width(None)
                .height(48.0)
                .placeholder("Search or type a model name")
                .label("Model name")
                .on_edit(move |s: &mut App| &mut form(s).pickers.entry(role).or_default().search)
                .build(&l.k),
        );
        let query = state.search.text.trim().to_lowercase();
        for option in options.iter().filter(|o| query.is_empty() || o.label.to_lowercase().contains(&query) || o.id.to_lowercase().contains(&query)) {
            let id = option.id.clone();
            let item = row()
                .w_full()
                .min_h(52.0)
                .py(12.0)
                .px(8.0)
                .gap(12.0)
                .center_y()
                .role("option")
                .label(option.label.clone())
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .child(text(option.label.clone()).font(l.k.ty.ui).color(l.text).wrap())
                        .child(text(option.detail.clone()).font(l.k.ty.small).color(l.text3).wrap()),
                )
                .child(rune::icon(&rune_kit::glyphs::CHECK, 18.0).color(l.amber).opacity(if option.id == current { 1.0 } else { 0.0 }));
            list = list.child(l.k.interactive(item).on_press_tx(move |s: &mut App, tx: &mut Tx| {
                form(s).edited();
                *form(s).slot(role).1 = rune::TextEdit::new(id.clone());
                form(s).pickers.entry(role).or_default().close();
                tx.blur();
                s.save_settings();
            }));
        }
        super::phone_sheet(&format!("{role}-model"), label, list, move |s| form(s).pickers.entry(role).or_default().close(), l, layer);
    }
    let trigger = row()
        .w_full()
        .min_h(48.0)
        .px(12.0)
        .py(8.0)
        .gap(8.0)
        .radius(8.0)
        .fill(l.raised)
        .center_y()
        .role("button")
        .label(label.to_owned())
        .child(
            text(if current.is_empty() { "Choose or type a model".to_owned() } else { current.to_owned() })
                .font(l.k.ty.ui)
                .color(l.text)
                .wrap()
                .grow(1.0)
                .min_w(0.0),
        )
        .child(rune::icon(l.k.glyph.chevron_down, 16.0).color(l.text3));
    l.k.interactive(trigger).on_press_tx(move |s: &mut App, tx: &mut Tx| {
        tx.blur();
        let state = form(s).pickers.entry(role).or_default();
        state.open = true;
        state.search = rune::TextEdit::new("");
    })
}

fn sync_row(s: &App, f: &SettingsForm, l: &Look) -> El {
    let k = &l.k;
    let on = s.settings.sync_keys;
    let mut r = col().w_full().gap(6.0).child(
        row()
            .w_full()
            .gap(10.0)
            .center_y()
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .gap(2.0)
                    .child(text("Sync with iCloud Keychain").font(k.ty.ui).color(l.text))
                    .child(text("Your keys follow you to your other Macs and your iPhone.").font(k.ty.small).color(l.text3).wrap()),
            )
            .child(if f.syncing { Spinner::new().size(14.0).build(k) } else { el() })
            .child(
                Switch::new(on)
                    .label("Sync with iCloud Keychain")
                    .hide_label()
                    .disabled(f.syncing)
                    .on_toggle(move |s: &mut App| s.set_key_sync(!on))
                    .build(k)
                    .key("sync-keys"),
            ),
    );
    match &f.sync {
        Some(Err(e @ (SyncError::Unsigned | SyncError::Failed(_)))) => r = r.child(text(e.to_string()).font(k.ty.small).color(l.text2).wrap()),
        Some(Ok(n)) if *n > 0 => r = r.child(text(format!("Moved {n} {}", if *n == 1 { "key" } else { "keys" })).font(k.ty.small).color(l.text3)),
        _ => {}
    }
    r
}

/// One provider's key: status, an optional base URL, a secure field with Save or Check, a
/// remove button and a hint.
fn provider_key(s: &App, f: &SettingsForm, i: usize, kind: ProviderKind, edit: &rune::TextEdit, l: &Look) -> El {
    let k = &l.k;
    let saved = f.has_provider_key(kind);
    let check = f.checks.get(kind.as_str());
    let url_changed = kind == ProviderKind::OpenAiCompatible && f.compatible_base_url.text.trim() != s.settings.compatible_base_url;
    let pasted = !edit.text.trim().is_empty();
    let mut c = col()
        .key(format!("provider-{}", kind.as_str()))
        .w_full()
        .gap(8.0)
        .padding(14.0)
        .radius(8.0)
        .border(1.0, l.line)
        .child(row().w_full().justify_between().center_y().child(text(kind.label()).font(k.ty.ui_medium).color(l.text)).child(status(check, saved, l)));
    if kind == ProviderKind::OpenAiCompatible {
        c = c.child(
            TextInput::new(&f.compatible_base_url)
                .width(None)
                .label("Base URL")
                .placeholder("https://openrouter.ai/api/v1")
                .on_edit(|s: &mut App| &mut form(s).compatible_base_url)
                .on_change(|s: &mut App, _: &mut Tx| {
                    let f = form(s);
                    f.edited();
                    f.invalidate_check(ProviderKind::OpenAiCompatible.as_str());
                })
                .build(k)
                .min_h(if l.size != theme::Size::Desk { 44.0 } else { 0.0 })
                .key("compatible-url"),
        );
    }
    let mut field = row().w_full().gap(8.0).center_y().child(
        col().grow(1.0).min_w(0.0).child(
            TextInput::new(edit)
                .label(format!("{} API key", kind.label()))
                .width(None)
                .placeholder(if saved { "Replace saved key" } else { "Paste API key" })
                .secure()
                .on_edit(move |s: &mut App| &mut form(s).keys[i].1)
                .on_change(move |s: &mut App, _: &mut Tx| {
                    let f = form(s);
                    f.edited();
                    f.invalidate_check(kind.as_str());
                })
                .on_submit(move |s: &mut App, _: &mut Tx| s.save_and_check_key(kind))
                .build(k)
                .min_h(if l.size != theme::Size::Desk { 44.0 } else { 0.0 })
                .key(format!("key-{}", kind.as_str())),
        ),
    );
    field = field.child(
        Button::new(if check == Some(&Check::Checking) {
            "Checking…"
        } else if pasted || url_changed {
            "Save & check"
        } else {
            "Check key"
        })
        .disabled(!(pasted || url_changed || saved || kind == ProviderKind::OpenAiCompatible) || check == Some(&Check::Checking))
        .on_press(move |s: &mut App| s.save_and_check_key(kind))
        .build(k)
        .min_h(if l.size != theme::Size::Desk { 44.0 } else { 0.0 })
        .min_w(if l.size != theme::Size::Desk { 44.0 } else { 0.0 })
        .key(format!("check-{}", kind.as_str())),
    );
    if saved {
        field = field.child(remove_button(engine_settings::provider_secret(kind.as_str()), l));
    }
    if pasted {
        c = c.child(text("New key not saved yet").font(k.ty.small).color(l.text2));
    }
    c = c.child(field).child(text(key_hint(kind)).font(k.ty.small).color(l.text3).wrap());
    if let Some(e) = error_of(check) {
        c = c.child(text(e.to_owned()).font(k.ty.small).color(l.danger).wrap());
    }
    c
}

fn remove_button(name: String, l: &Look) -> El {
    IconButton::new(&icons::TRASH)
        .variant(IconButtonVariant::GhostMuted)
        .control_size(if l.phone() { ControlSize::ExtraLarge } else { ControlSize::Small })
        .label("Remove key")
        .on_press(move |s: &mut App| s.remove_key(name.clone()))
        .build(&l.k)
        .min_h(if l.size != theme::Size::Desk { 44.0 } else { 0.0 })
        .min_w(if l.size != theme::Size::Desk { 44.0 } else { 0.0 })
        .key("remove-key")
}

fn elevenlabs_key(f: &SettingsForm, l: &Look) -> El {
    let k = &l.k;
    let name = engine_settings::provider_secret("elevenlabs");
    let saved = f.has_key(&name);
    let check = f.checks.get("elevenlabs");
    let pasted = !f.elevenlabs.text.trim().is_empty();
    let mut field = row().w_full().gap(8.0).center_y().child(
        col().grow(1.0).min_w(0.0).child(
            TextInput::new(&f.elevenlabs)
                .label("ElevenLabs API key")
                .width(None)
                .placeholder(if saved { "Replace saved key" } else { "Paste API key" })
                .secure()
                .on_edit(|s: &mut App| &mut form(s).elevenlabs)
                .on_change(|s: &mut App, _: &mut Tx| {
                    let f = form(s);
                    f.edited();
                    f.invalidate_check("elevenlabs");
                })
                .on_submit(|s: &mut App, _: &mut Tx| s.save_and_check_elevenlabs())
                .build(k)
                .min_h(if l.size != theme::Size::Desk { 44.0 } else { 0.0 })
                .key("key-elevenlabs"),
        ),
    );
    field = field.child(
        Button::new(if check == Some(&Check::Checking) {
            "Checking…"
        } else if pasted {
            "Save & check"
        } else {
            "Check key"
        })
        .disabled(!(pasted || saved) || check == Some(&Check::Checking))
        .on_press(|s: &mut App| s.save_and_check_elevenlabs())
        .build(k)
        .min_h(if l.size != theme::Size::Desk { 44.0 } else { 0.0 })
        .min_w(if l.size != theme::Size::Desk { 44.0 } else { 0.0 })
        .key("check-elevenlabs"),
    );
    if saved {
        field = field.child(remove_button(name, l));
    }
    let mut c = col()
        .key("provider-elevenlabs")
        .w_full()
        .gap(8.0)
        .padding(14.0)
        .radius(8.0)
        .border(1.0, l.line)
        .child(row().w_full().justify_between().center_y().child(text("ElevenLabs").font(k.ty.ui_medium).color(l.text)).child(status(check, saved, l)))
        .child(field)
        .child(text("Only used for podcast voices.").font(k.ty.small).color(l.text3));
    if let Some(e) = error_of(check) {
        c = c.child(text(e.to_owned()).font(k.ty.small).color(l.danger).wrap());
    }
    c
}

fn server_section(s: &App, f: &SettingsForm, l: &Look) -> El {
    let k = &l.k;
    let has_password = f.has_key(engine_settings::SERVER_PASSWORD);
    let check = f.checks.get("server");
    let busy = check == Some(&Check::Checking);
    let mut c = section("Connect to a server", l)
        .child(
            text("Enter the address you use to open Open Notebook. Your current notebooks stay active until the connection succeeds.")
                .font(k.ty.ui)
                .color(l.text2)
                .wrap(),
        )
        .child(field_row(
            "Server address",
            TextInput::new(&f.server_url)
                .width(None)
                .placeholder("https://notebook.example.ts.net:8502")
                .on_edit(|s: &mut App| &mut form(s).server_url)
                .on_change(|s: &mut App, _: &mut Tx| form(s).connection_edited())
                .on_submit(|s: &mut App, _: &mut Tx| s.save_and_test_server())
                .build(k)
                .min_h(44.0)
                .key("server-url"),
            l,
        ))
        .child(field_row(
            "Server password",
            TextInput::new(&f.server_password)
                .width(None)
                .secure()
                .placeholder(if has_password { "Password saved. Leave empty to keep it" } else { "Only if your server requires one" })
                .on_edit(|s: &mut App| &mut form(s).server_password)
                .on_change(|s: &mut App, _: &mut Tx| form(s).connection_edited())
                .on_submit(|s: &mut App, _: &mut Tx| s.save_and_test_server())
                .build(k)
                .min_h(44.0)
                .key("server-password"),
            l,
        ));
    if has_password {
        c = c.child(text("Password saved on this device").font(k.ty.small).color(l.text2));
    }
    let mut actions = row().w_full().gap(8.0).center_y().flex_wrap().child(
        Button::new(if busy { "Connecting…" } else { "Save and connect" })
            .variant(ButtonVariant::Contrast)
            .disabled(f.server_url.text.trim().is_empty() || busy)
            .on_press(App::save_and_test_server)
            .build(k)
            .min_h(44.0)
            .key("save-connect"),
    );
    if has_password {
        actions = actions.child(
            Button::new("Forget password")
                .variant(ButtonVariant::Ghost)
                .disabled(busy)
                .on_press(App::forget_server_password)
                .build(k)
                .min_h(44.0)
                .key("forget-password"),
        );
    }
    c = c.child(actions);
    if let Some(check) = check {
        c = c.child(operation_feedback(check, "Connecting and checking access to your notebooks…", l).key("server-feedback"));
    }
    if s.settings.mode == Mode::Remote && matches!(check, Some(Check::Working(_))) {
        c = c.child(text("Models and provider keys for server notebooks are managed on the Server page.").font(k.ty.small).color(l.text3).wrap());
    }
    c
}
