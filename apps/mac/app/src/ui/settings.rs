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
        ProviderKind::Anthropic => "Claude models for answers. Anthropic has no embeddings, so pair it with another provider for search.",
        ProviderKind::OpenAi | ProviderKind::Gemini => "Answers and embeddings from one key.",
        ProviderKind::OpenAiCompatible => "OpenRouter, Groq, or a model running on this Mac in LM Studio or Ollama. The key is optional for local servers.",
    }
}

fn role_label(role: &str) -> &'static str {
    match role {
        "chat" => "Chat model",
        "transformation" => "Transformation model",
        "embedding" => "Embedding model",
        _ => "Speech to text",
    }
}

fn role_help(role: &str) -> Option<&'static str> {
    match role {
        "embedding" => Some("Without one, search matches words instead of meaning. Changing it re-indexes new sources only."),
        "transformation" => Some("Writes insights and study decks. Leave empty to use the chat model."),
        "transcription" => Some("Turns audio and video sources into text. Up to 25 MB with OpenAI, 19 MB with Gemini."),
        _ => None,
    }
}

pub fn view(s: &App, l: &Look, layer: &mut Vec<El>) -> El {
    let k = &l.k;
    let Some(f) = &s.form else { return col() };
    let local = s.settings.mode == Mode::Local;
    let mode = super::pages::choices(
        &[if cfg!(target_os = "ios") { "On this iPhone" } else { "On this Mac" }, if l.phone() { "Server" } else { "Open Notebook server" }],
        Some(if local { 0 } else { 1 }),
        "Where notebooks live",
        l,
        |s: &mut App, i, _: &mut Tx| {
            s.settings.mode = if i == 0 { Mode::Local } else { Mode::Remote };
            s.save_settings();
        },
    )
    .key("mode");
    let mut c = col()
        .key("settings-body")
        .w_full()
        .max_w(560.0)
        .gap(if l.phone() { 32.0 } else { 18.0 })
        .px(l.pad())
        .pt(l.pad_top())
        .pb(l.pad_top() + l.inset_bottom)
        .child(text("Settings").font(theme::READ_TITLE).color(l.text))
        .child(
            section("Notebooks", l).child(row().w_full().child(mode)).child(
                text(if local {
                    format!("Everything stays on {}. Questions go to the AI provider you pick, with your own key.", theme::THIS_DEVICE)
                } else {
                    "Notebooks live on your server, which can use local models.".to_owned()
                })
                .font(k.ty.ui)
                .color(l.text2)
                .wrap(),
            ),
        );
    if local {
        let mut models = section("Models", l).child(
            text("Answers use the chat model. The embedding model indexes sources so questions find the right passages.")
                .font(k.ty.small)
                .color(l.text3)
                .wrap(),
        );
        for role in crate::state::MODEL_ROLES {
            models = models.child(model_row(f, role, l, layer));
        }
        models = models.child(rebuild_row(f, l)).child(
            row()
                .w_full()
                .gap(10.0)
                .center_y()
                .child(
                    Button::new("Save models")
                        .variant(ButtonVariant::Contrast)
                        .on_press(|s: &mut App| s.save_settings())
                        .build(k)
                        .min_h(if l.size != theme::Size::Desk { 44.0 } else { 0.0 })
                        .min_w(if l.size != theme::Size::Desk { 44.0 } else { 0.0 })
                        .key("save-settings"),
                )
                .child(text(if f.saved { "Saved" } else { "" }).font(k.ty.ui).color(l.text3)),
        );
        c = c.child(models).child(podcasts(l));
        let mut keys = section("API keys", l)
            .child(
                text("Keys are kept in a file only you can read, in this app's folder, and only sent to their provider. A saved key is never shown.")
                    .font(k.ty.small)
                    .color(l.text3)
                    .wrap(),
            )
            .child(sync_row(s, f, l));
        for (i, (kind, edit)) in f.keys.iter().enumerate() {
            keys = keys.child(provider_key(s, f, i, *kind, edit, l));
        }
        c = c.child(keys.child(elevenlabs_key(f, l)));
    } else {
        c = c.child(server_section(s, f, l)).child(podcasts(l));
    }
    let appearance = ["system", "light", "dark"];
    c = c.child(
        section("Appearance", l).child(
            row().w_full().child(
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
        ),
    );
    col().key("settings").w_full().items_center().child(c).scroll(ScrollSpec { stick_bottom: false }).grow(1.0).min_h(0.0)
}

fn section(title: &str, l: &Look) -> El {
    col().w_full().gap(if l.phone() { 16.0 } else { 10.0 }).child(text(title.to_owned()).font(l.k.ty.ui_medium).color(l.text))
}

fn field_row(label: &str, field: El, l: &Look) -> El {
    col().w_full().gap(4.0).child(text(label.to_owned()).font(l.k.ty.small).color(l.text2)).child(field)
}

/// A check's status: a spinner, "Working, …" or "Not working".
fn status(check: Option<&Check>, saved: bool, l: &Look) -> El {
    let k = &l.k;
    match check {
        Some(Check::Checking) => Spinner::new().size(14.0).build(k),
        Some(Check::Working(what)) => text(what.clone()).font(k.ty.small).color(l.text2),
        Some(Check::Failed(_)) => text("Not working").font(k.ty.small).color(l.danger),
        None if saved => text("Key saved").font(k.ty.small).color(l.text2),
        None => el(),
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
        return field_row(label, text("Save an API key below first").font(k.ty.small).color(l.text3), l);
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
    if let Some(ModelList::Failed(e)) = list {
        c = c.child(text(format!("Could not list models: {e}")).font(k.ty.small).color(l.danger).wrap());
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
        .radius(12.0)
        .border(1.0, l.line)
        .child(row().w_full().justify_between().center_y().child(text(kind.label()).font(k.ty.ui_medium).color(l.text)).child(status(check, saved, l)));
    if kind == ProviderKind::OpenAiCompatible {
        c = c.child(
            TextInput::new(&f.compatible_base_url)
                .width(None)
                .label("Base URL")
                .placeholder("https://openrouter.ai/api/v1")
                .on_edit(|s: &mut App| &mut form(s).compatible_base_url)
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
                .placeholder(if saved { "Saved. Paste a new key to replace it" } else { "Paste API key" })
                .secure()
                .on_edit(move |s: &mut App| &mut form(s).keys[i].1)
                .on_submit(move |s: &mut App, _: &mut Tx| s.save_and_check_key(kind))
                .build(k)
                .min_h(if l.size != theme::Size::Desk { 44.0 } else { 0.0 })
                .key(format!("key-{}", kind.as_str())),
        ),
    );
    field = field.child(
        Button::new(if pasted || url_changed { "Save" } else { "Check" })
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
                .placeholder(if saved { "Saved. Paste a new key to replace it" } else { "Paste API key" })
                .secure()
                .on_edit(|s: &mut App| &mut form(s).elevenlabs)
                .on_submit(|s: &mut App, _: &mut Tx| s.save_and_check_elevenlabs())
                .build(k)
                .min_h(if l.size != theme::Size::Desk { 44.0 } else { 0.0 })
                .key("key-elevenlabs"),
        ),
    );
    field = field.child(
        Button::new(if pasted { "Save" } else { "Check" })
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
        .radius(12.0)
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
    let mut actions = row().w_full().gap(8.0).center_y().child(
        Button::new("Save and connect")
            .variant(ButtonVariant::Contrast)
            .disabled(f.server_url.text.trim().is_empty() || check == Some(&Check::Checking))
            .on_press(|s: &mut App| s.save_and_test_server())
            .build(k)
            .min_h(if l.size != theme::Size::Desk { 44.0 } else { 0.0 })
            .min_w(if l.size != theme::Size::Desk { 44.0 } else { 0.0 })
            .key("save-connect"),
    );
    if has_password {
        actions = actions.child(
            Button::new("Forget password")
                .variant(ButtonVariant::Ghost)
                .on_press(|s: &mut App| s.forget_server_password())
                .build(k)
                .min_h(if l.size != theme::Size::Desk { 44.0 } else { 0.0 })
                .min_w(if l.size != theme::Size::Desk { 44.0 } else { 0.0 })
                .key("forget-password"),
        );
    }
    actions = actions.child(el().grow(1.0));
    match check {
        Some(Check::Checking) => actions = actions.child(Spinner::new().size(14.0).build(k)),
        Some(Check::Working(what)) => actions = actions.child(text(what.clone()).font(k.ty.small).color(l.text2)),
        _ => {}
    }
    let mut c = section("Server", l)
        .child(text("The address you open Open Notebook at, for example its Tailscale address.").font(k.ty.small).color(l.text3).wrap())
        .child(field_row(
            "Address",
            TextInput::new(&f.server_url)
                .width(None)
                .placeholder("https://notebook.example.ts.net:8502")
                .on_edit(|s: &mut App| &mut form(s).server_url)
                .build(k)
                .min_h(if l.size != theme::Size::Desk { 44.0 } else { 0.0 })
                .key("server-url"),
            l,
        ))
        .child(field_row(
            "Password",
            TextInput::new(&f.server_password)
                .width(None)
                .secure()
                .placeholder(if has_password {
                    if cfg!(target_os = "ios") { "Saved on this iPhone" } else { "Saved on this Mac" }
                } else {
                    "Only if the server has one"
                })
                .on_edit(|s: &mut App| &mut form(s).server_password)
                .on_submit(|s: &mut App, _: &mut Tx| s.save_and_test_server())
                .build(k)
                .min_h(if l.size != theme::Size::Desk { 44.0 } else { 0.0 })
                .key("server-password"),
            l,
        ))
        .child(actions);
    if let Some(e) = error_of(check) {
        c = c.child(text(e.to_owned()).font(k.ty.small).color(l.danger).wrap());
    }
    if let Some(e) = &s.backend_error {
        c = c.child(text(e.clone()).font(k.ty.small).color(l.danger).wrap());
    }
    c
}
