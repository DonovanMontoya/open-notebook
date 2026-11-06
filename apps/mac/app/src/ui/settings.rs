//! Settings: where notebooks live (this Mac or a server), API keys and models.

use notebook_engine::settings::Mode;
use rune::{El, ScrollSpec, TextEdit, Tx, col, row, text};
use rune_kit::{Button, ButtonVariant, SegmentedControl, Select, TextInput};

use notebook_engine::providers::ProviderKind;

use crate::state::{App, SettingsForm};
use crate::theme::{self, Look};

fn form(s: &mut App) -> &mut SettingsForm {
    s.form.as_mut().expect("settings are open")
}

pub fn view(s: &App, l: &Look, layer: &mut Vec<El>) -> El {
    let k = &l.k;
    let Some(f) = &s.form else { return col() };
    let local = s.settings.mode == Mode::Local;
    let mode = SegmentedControl::new(&["On this Mac", "Open Notebook server"], Some(if local { 0 } else { 1 }))
        .label("Where notebooks live")
        .fit()
        .on_pick(|s: &mut App, i, _: &mut Tx| {
            s.settings.mode = if i == 0 { Mode::Local } else { Mode::Remote };
            s.save_settings();
        })
        .build(k)
        .key("mode");
    let mut c =
        col().key("settings-body").w_full().max_w(560.0).gap(18.0).px(24.0).py(28.0).child(text("Settings").font(theme::READ_TITLE).color(l.text)).child(
            section("Notebooks", l).child(mode).child(
                text(if local {
                    "Everything stays on this Mac. Questions go to the AI provider you pick, with your own key."
                } else {
                    "Notebooks live on your server, which can use local models."
                })
                .font(k.ty.ui)
                .color(l.text2)
                .wrap(),
            ),
        );
    if local {
        let mut keys = section("API keys", l).child(text("Kept in a file only you can read, in this app's folder.").font(k.ty.small).color(l.text3).wrap());
        for (i, (id, edit)) in f.keys.iter().enumerate() {
            let label = id.label();
            keys = keys.child(field_row(
                label,
                TextInput::new(edit)
                    .label(label)
                    .width(None)
                    .placeholder("Not set")
                    .secure()
                    .on_edit(move |s: &mut App| &mut form(s).keys[i].1)
                    .build(k)
                    .key(format!("key-{}", id.as_str())),
                l,
            ));
        }
        keys = keys.child(field_row(
            "ElevenLabs (podcast voices)",
            TextInput::new(&f.elevenlabs)
                .label("ElevenLabs")
                .width(None)
                .placeholder("Not set")
                .secure()
                .on_edit(|s: &mut App| &mut form(s).elevenlabs)
                .build(k)
                .key("key-elevenlabs"),
            l,
        ));
        keys = keys.child(field_row(
            "OpenAI-compatible base URL",
            TextInput::new(&f.compatible_base_url).width(None).on_edit(|s: &mut App| &mut form(s).compatible_base_url).build(k).key("compatible-url"),
            l,
        ));
        c = c.child(keys);
        let models = section("Models", l)
            .child(model_row("Chat", "chat", f.chat_provider, &f.chat_model, f.open_select == Some("chat"), |f| &mut f.chat_model, |f, p| f.chat_provider = p, l, layer))
            .child(model_row(
                "Embeddings (search)",
                "embedding",
                f.embedding_provider,
                &f.embedding_model,
                f.open_select == Some("embedding"),
                |f| &mut f.embedding_model,
                |f, p| f.embedding_provider = p,
                l,
                layer,
            ))
            .child(model_row(
                "Speech to text (audio and video sources)",
                "transcription",
                f.transcription_provider,
                &f.transcription_model,
                f.open_select == Some("transcription"),
                |f| &mut f.transcription_model,
                |f, p| f.transcription_provider = p,
                l,
                layer,
            ))
            .child(text("For example claude-sonnet-4-5, gpt-5.1 or gemini-2.5-pro for chat, and text-embedding-3-small for embeddings, whisper-1 or gpt-4o-transcribe for speech.").font(k.ty.small).color(l.text3).wrap());
        c = c.child(models);
    } else {
        c = c.child(
            section("Server", l)
                .child(field_row(
                    "Server URL",
                    TextInput::new(&f.server_url).width(None).on_edit(|s: &mut App| &mut form(s).server_url).build(k).key("server-url"),
                    l,
                ))
                .child(field_row(
                    "Password",
                    TextInput::new(&f.server_password)
                        .width(None)
                        .secure()
                        .placeholder("If the server has one")
                        .on_edit(|s: &mut App| &mut form(s).server_password)
                        .build(k)
                        .key("server-password"),
                    l,
                )),
        );
    }
    let appearance = ["system", "light", "dark"];
    c = c.child(
        section("Appearance", l).child(
            SegmentedControl::new(&["System", "Light", "Dark"], appearance.iter().position(|a| *a == s.settings.appearance))
                .label("Appearance")
                .fit()
                .on_pick(move |s: &mut App, i, _: &mut Tx| {
                    s.settings.appearance = appearance[i].into();
                    s.save_settings();
                })
                .build(k)
                .key("appearance"),
        ),
    );
    c = c.child(
        row()
            .w_full()
            .gap(10.0)
            .center_y()
            .child(Button::new("Save").variant(ButtonVariant::Contrast).on_press(|s: &mut App| s.save_settings()).build(k).key("save-settings"))
            .child(text(if f.saved { "Saved" } else { "" }).font(k.ty.ui).color(l.text3)),
    );
    col().key("settings").w_full().items_center().child(c).scroll(ScrollSpec { stick_bottom: false }).grow(1.0).min_h(0.0)
}

fn section(title: &str, l: &Look) -> El {
    col().w_full().gap(10.0).child(text(title.to_owned()).font(l.k.ty.ui_medium).color(l.text))
}

fn field_row(label: &str, field: El, l: &Look) -> El {
    col().w_full().gap(4.0).child(text(label.to_owned()).font(l.k.ty.small).color(l.text2)).child(field)
}

#[allow(clippy::too_many_arguments)]
fn model_row(
    label: &str,
    key: &'static str,
    provider: ProviderKind,
    model: &TextEdit,
    open: bool,
    edit: fn(&mut SettingsForm) -> &mut TextEdit,
    set_provider: fn(&mut SettingsForm, ProviderKind),
    l: &Look,
    layer: &mut Vec<El>,
) -> El {
    let k = &l.k;
    let names: Vec<&str> = ProviderKind::ALL.iter().map(|p| p.label()).collect();
    let select = Select::new(format!("{key}-provider"), &names)
        .label(format!("{label} provider"))
        .current(Some(provider.label()))
        .width(190.0)
        .open(open)
        .on_toggle(move |s: &mut App| {
            let f = form(s);
            f.open_select = if f.open_select == Some(key) { None } else { Some(key) };
        })
        .on_close(|s: &mut App| form(s).open_select = None)
        .on_choose(move |s: &mut App, name: &str, _: &mut Tx| {
            if let Some(p) = ProviderKind::ALL.into_iter().find(|p| p.label() == name) {
                set_provider(form(s), p);
            }
            form(s).open_select = None;
        })
        .build(k, layer)
        .key(format!("{key}-provider"));
    let input = TextInput::new(model).width(None).placeholder("Model name").on_edit(move |s: &mut App| edit(form(s))).build(k).key(format!("{key}-model"));
    field_row(label, row().w_full().gap(8.0).child(select).child(rune::col().grow(1.0).min_w(0.0).child(input)), l)
}
