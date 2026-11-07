//! The server admin page: default models, models, API keys and background jobs.

use rune::{El, ScrollSpec, Tx, col, el, row, text};
use rune_kit::{Button, ButtonVariant, Checkbox, ControlSize, Select, TextInput};

use crate::format;
use crate::server::{
    CAPTION_LANGUAGES, KEY_PROVIDERS, MODEL_TYPES, NewKey, PROCESSING, PROCESSING_SWITCHES, SLOTS, ServerPage, cancellable, job_label, type_label,
};
use crate::state::App;
use crate::theme::{self, Look};

fn page(s: &mut App) -> &mut ServerPage {
    s.server.as_mut().expect("server page")
}

fn section(title: &str, intro: &str, l: &Look) -> El {
    col()
        .w_full()
        .gap(10.0)
        .pt(8.0)
        .child(text(title.to_owned()).font(theme::READ_HEADING).color(l.text))
        .child(text(intro.to_owned()).font(l.k.ty.ui).color(l.text2).wrap())
}

fn small(label: &str, key: String, l: &Look, f: impl Fn(&mut App) + 'static) -> El {
    Button::new(label.to_owned()).variant(ButtonVariant::Ghost).control_size(ControlSize::Small).on_press(f).build(&l.k).key(key)
}

pub fn view(s: &App, l: &Look, layer: &mut Vec<El>) -> El {
    let k = &l.k;
    let Some(p) = &s.server else { return col() };
    let mut c = col()
        .w_full()
        .max_w(720.0)
        .gap(14.0)
        .px(l.pad())
        .py(l.pad_top())
        .child(text("Server").font(theme::READ_TITLE).color(l.text))
        .child(text(format!("Admin for {}.", s.settings.server_url)).font(k.ty.ui).color(l.text2).wrap());

    // Default models, one per job.
    let mut defaults = section("Default models", "Which model the server uses for each job.", l);
    for (slot, label, kind) in SLOTS {
        let models: Vec<&notebook_engine::types::ServerModel> = p.models.iter().filter(|m| m.kind == kind).collect();
        let mut names = vec!["Not set".to_owned()];
        names.extend(models.iter().map(|m| format!("{} · {}", m.name, m.provider)));
        let ids: Vec<String> = models.iter().map(|m| m.id.clone()).collect();
        let current = p.defaults.get(slot).cloned().flatten().and_then(|id| ids.iter().position(|x| *x == id)).map(|i| names[i + 1].clone());
        let select = Select::new(format!("slot-{slot}"), &names)
            .label(label)
            .current(Some(current.as_deref().unwrap_or("Not set")))
            .width(300.0)
            .open(p.open_select.as_deref() == Some(slot))
            .on_toggle(move |s: &mut App| {
                let p = page(s);
                p.open_select = if p.open_select.as_deref() == Some(slot) { None } else { Some(slot.into()) };
            })
            .on_close(|s: &mut App| page(s).open_select = None)
            .on_choose({
                let names = names.clone();
                move |s: &mut App, name: &str, _: &mut Tx| {
                    let model = names.iter().position(|n| n == name).and_then(|i| i.checked_sub(1)).and_then(|i| ids.get(i).cloned());
                    s.set_default(slot, model);
                }
            })
            .build(k, layer);
        defaults = defaults.child(row().w_full().gap(12.0).center_y().child(text(label).font(k.ty.ui).color(l.text).w(220.0)).child(select));
    }
    c = c.child(defaults);

    // Models, by type.
    let mut models = section("Models", "Models the server knows about.", l);
    for (kind, label) in MODEL_TYPES {
        let of_kind: Vec<_> = p.models.iter().filter(|m| m.kind == kind).collect();
        if of_kind.is_empty() {
            continue;
        }
        models = models.child(text(label).font(k.ty.ui_medium).color(l.text2).pt(4.0));
        for m in of_kind {
            let (t, d, name) = (m.id.clone(), m.id.clone(), m.name.clone());
            let mut r = row().key(format!("m-{}", m.id)).w_full().gap(8.0).py(2.0).center_y().child(
                row()
                    .grow(1.0)
                    .min_w(0.0)
                    .gap(8.0)
                    .center_y()
                    .child(text(m.name.clone()).font(k.ty.ui).color(l.text).max_lines(1))
                    .child(text(m.provider.clone()).font(k.ty.small).color(l.text3)),
            );
            if let Some(note) = p.notes.get(&m.id) {
                r = r.child(text(note.clone()).font(k.ty.small).color(l.text2).max_w(220.0).max_lines(2));
            }
            r = r.child(small("Test", "test".into(), l, move |s| s.test_model(t.clone()))).child(small("Remove", "remove".into(), l, move |s| {
                let id = d.clone();
                s.ask_to("Remove", format!("Remove {name}?"), move |s| s.delete_model(id.clone()));
            }));
            models = models.child(r);
        }
    }
    models = models.child(add_model(p, l, layer)).child(row().child(small("Find models from saved keys", "sync".into(), l, |s: &mut App| s.sync_models())));
    c = c.child(models);

    // API keys.
    let mut keys = section("API keys", "Provider keys stored on the server, encrypted.", l);
    for cr in &p.credentials {
        let (t, g, d) = (cr.id.clone(), cr.id.clone(), cr.id.clone());
        let mut detail = cr.provider.clone();
        if !cr.modalities.is_empty() {
            detail += &format!(", {}", cr.modalities.iter().map(|m| type_label(m).to_lowercase()).collect::<Vec<_>>().join(", "));
        }
        if let Some(n) = cr.model_count.filter(|n| *n > 0) {
            detail += &format!(", {n} models");
        }
        if let Some(e) = &cr.decryption_error {
            detail = e.clone();
        }
        let mut r = row().key(format!("c-{}", cr.id)).w_full().gap(8.0).py(4.0).center_y().child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .child(text(cr.name.clone()).font(k.ty.ui).color(l.text).max_lines(1))
                .child(text(detail).font(k.ty.small).color(if cr.decryption_error.is_some() { l.danger } else { l.text3 })),
        );
        if let Some(note) = p.notes.get(&cr.id) {
            r = r.child(text(note.clone()).font(k.ty.small).color(l.text2).max_w(200.0).max_lines(2));
        }
        r = r
            .child(small("Test", "test".into(), l, move |s| s.test_credential(t.clone())))
            .child(small("Add models", "register".into(), l, move |s| s.register_models(g.clone())))
            .child(small("Remove", "remove".into(), l, move |s| {
                let id = d.clone();
                s.ask_to("Remove", "Remove this key from the server?", move |s| s.delete_credential(id.clone()));
            }));
        keys = keys.child(r);
    }
    match &p.adding {
        None => keys = keys.child(row().child(small("Add a key", "add-key".into(), l, |s| page(s).adding = Some(NewKey::default())))),
        Some(a) => keys = keys.child(new_key(p, a, l, layer)),
    }
    c = c.child(keys);

    c = c.child(processing(p, l, layer));

    // Jobs, refreshed every few seconds while the page is open.
    let mut jobs = section("Background jobs", "Source processing, embeddings and podcasts on the server.", l).child(row().gap(8.0).child(small(
        "Refresh",
        "jobs-refresh".into(),
        l,
        |s| s.load_jobs(),
    )));
    if p.jobs.is_empty() {
        jobs = jobs.child(text("Nothing running").font(k.ty.ui).color(l.text3));
    }
    for j in &p.jobs {
        let id = j.job_id.clone().or_else(|| j.id.clone()).unwrap_or_default();
        let mut detail = j.status.clone();
        if let Some(created) = &j.created {
            detail += &format!(", {}", format::relative_time(created));
        }
        let mut r = row()
            .key(format!("j-{id}"))
            .w_full()
            .gap(8.0)
            .center_y()
            .child(text(job_label(j)).font(k.ty.ui).color(l.text).shrink().min_w(0.0).max_lines(1))
            .child(text(detail).font(k.ty.small).color(l.text3))
            .child(el().grow(1.0));
        if let Some(e) = j.error_message.as_deref().filter(|e| !e.is_empty()) {
            r = r.child(text("Error").font(k.ty.small).color(l.danger).help(e.to_owned()));
        }
        if cancellable(&j.status) {
            let cancel = id.clone();
            r = r.child(small("Cancel", "cancel".into(), l, move |s| s.cancel_job(cancel.clone())));
        }
        jobs = jobs.child(r);
    }
    c = c.child(jobs);
    col().key("server").w_full().items_center().child(c).scroll(ScrollSpec { stick_bottom: false }).grow(1.0).min_h(0.0)
}

fn new_key(p: &ServerPage, a: &NewKey, l: &Look, layer: &mut Vec<El>) -> El {
    let k = &l.k;
    fn adding(s: &mut App) -> &mut NewKey {
        page(s).adding.as_mut().expect("adding a key")
    }
    let providers: Vec<String> = KEY_PROVIDERS.iter().map(|p| p.to_string()).collect();
    let provider = Select::new("key-provider", &providers)
        .label("Provider")
        .placeholder("Provider")
        .current(a.provider.as_deref())
        .width(220.0)
        .open(p.open_select.as_deref() == Some("provider"))
        .on_toggle(|s: &mut App| {
            let p = page(s);
            p.open_select = if p.open_select.as_deref() == Some("provider") { None } else { Some("provider".into()) };
        })
        .on_close(|s: &mut App| page(s).open_select = None)
        .on_choose(|s: &mut App, name: &str, _: &mut Tx| {
            adding(s).provider = Some(name.into());
            page(s).open_select = None;
        })
        .build(k, layer);
    let mut uses = row().w_full().gap(14.0).flex_wrap().child(text("Used for").font(k.ty.small).color(l.text2));
    for (kind, label) in MODEL_TYPES {
        uses = uses.child(
            Checkbox::new(a.modalities.iter().any(|m| m == kind))
                .label(label)
                .on_toggle(move |s: &mut App| {
                    let m = &mut adding(s).modalities;
                    if let Some(i) = m.iter().position(|x| x == kind) {
                        m.remove(i);
                    } else {
                        m.push(kind.into());
                    }
                })
                .build(k)
                .key(format!("use-{kind}")),
        );
    }
    col()
        .key("new-key")
        .w_full()
        .gap(8.0)
        .padding(14.0)
        .radius(12.0)
        .border(1.0, l.line_strong)
        .child(provider)
        .child(TextInput::new(&a.name).width(None).placeholder("Name").label("Key name").on_edit(|s: &mut App| &mut adding(s).name).build(k).key("key-name"))
        .child(TextInput::new(&a.api_key).width(None).secure().placeholder("API key").on_edit(|s: &mut App| &mut adding(s).api_key).build(k).key("key-value"))
        .child(
            TextInput::new(&a.base_url)
                .width(None)
                .placeholder("Base URL, only for self-hosted or compatible providers")
                .on_edit(|s: &mut App| &mut adding(s).base_url)
                .build(k)
                .key("key-url"),
        )
        .child(uses)
        .child(
            row()
                .w_full()
                .gap(8.0)
                .justify_end()
                .child(el().grow(1.0))
                .child(Button::new("Cancel").variant(ButtonVariant::Ghost).on_press(|s: &mut App| page(s).adding = None).build(k).key("key-cancel"))
                .child(
                    Button::new("Save key")
                        .variant(ButtonVariant::Contrast)
                        .disabled(!a.ready())
                        .on_press(|s: &mut App| s.add_credential())
                        .build(k)
                        .key("key-save"),
                ),
        )
}

/// A model added by hand: provider, type (what that provider supports), name.
fn add_model(p: &ServerPage, l: &Look, layer: &mut Vec<El>) -> El {
    let k = &l.k;
    let n = &p.new_model;
    let toggle = |which: &'static str| {
        move |s: &mut App| {
            let p = page(s);
            p.open_select = if p.open_select.as_deref() == Some(which) { None } else { Some(which.into()) };
        }
    };
    let provider = Select::new("model-provider", &p.providers)
        .label("Provider")
        .placeholder("Provider")
        .current(n.provider.as_deref())
        .width(170.0)
        .open(p.open_select.as_deref() == Some("model-provider"))
        .on_toggle(toggle("model-provider"))
        .on_close(|s: &mut App| page(s).open_select = None)
        .on_choose(|s: &mut App, name: &str, _: &mut Tx| {
            let p = page(s);
            p.open_select = None;
            p.new_model.provider = Some(name.into());
            let types = p.supported_types.get(name);
            if types.is_some_and(|t| !t.contains(&p.new_model.kind))
                && let Some(first) = types.and_then(|t| t.first())
            {
                p.new_model.kind = first.clone();
            }
        })
        .build(k, layer);
    let types: Vec<String> = match n.provider.as_ref().and_then(|x| p.supported_types.get(x)) {
        Some(t) => t.clone(),
        None => MODEL_TYPES.iter().map(|(t, _)| t.to_string()).collect(),
    };
    let labels: Vec<String> = types.iter().map(|t| type_label(t).to_owned()).collect();
    let kind = Select::new("model-type", &labels)
        .label("Type")
        .current(Some(type_label(&n.kind)))
        .width(160.0)
        .open(p.open_select.as_deref() == Some("model-type"))
        .on_toggle(toggle("model-type"))
        .on_close(|s: &mut App| page(s).open_select = None)
        .on_choose(move |s: &mut App, label: &str, _: &mut Tx| {
            let p = page(s);
            p.open_select = None;
            if let Some(t) = types.iter().find(|t| type_label(t) == label) {
                p.new_model.kind = t.clone();
            }
        })
        .build(k, layer);
    let name = TextInput::new(&n.name)
        .width(None)
        .placeholder("Model name")
        .label("Model name")
        .on_edit(|s: &mut App| &mut page(s).new_model.name)
        .on_submit(|s: &mut App, _: &mut Tx| s.add_model())
        .build(k)
        .key("model-name");
    row().key("add-model").w_full().gap(8.0).pt(6.0).center_y().child(provider).child(kind).child(col().grow(1.0).min_w(0.0).child(name)).child(
        Button::new("Add").disabled(n.provider.is_none() || n.name.text.trim().is_empty()).on_press(|s: &mut App| s.add_model()).build(k).key("model-add"),
    )
}

/// How the server reads documents and pages, and its search index.
fn processing(p: &ServerPage, l: &Look, layer: &mut Vec<El>) -> El {
    let k = &l.k;
    let mut c = section("Processing", "How the server reads documents and pages.", l);
    let Some(settings) = &p.settings else {
        return c.child(text("Loading").font(k.ty.ui).color(l.text3));
    };
    for (key, label, options) in PROCESSING {
        let names: Vec<&str> = options.iter().map(|(_, n)| *n).collect();
        let value = settings.get(key).and_then(|v| v.as_str()).unwrap_or("");
        let current = options.iter().find(|(v, _)| *v == value).map(|(_, n)| *n);
        let select = Select::new(format!("proc-{key}"), &names)
            .label(label)
            .placeholder("Not set")
            .current(current)
            .width(220.0)
            .open(p.open_select.as_deref() == Some(key))
            .on_toggle(move |s: &mut App| {
                let p = page(s);
                p.open_select = if p.open_select.as_deref() == Some(key) { None } else { Some(key.into()) };
            })
            .on_close(|s: &mut App| page(s).open_select = None)
            .on_choose(move |s: &mut App, name: &str, _: &mut Tx| {
                if let Some((v, _)) = options.iter().find(|(_, n)| *n == name) {
                    s.set_server_setting(key, serde_json::Value::String((*v).into()));
                }
            })
            .build(k, layer);
        c = c.child(row().w_full().gap(12.0).center_y().child(text(label).font(k.ty.ui).color(l.text).w(220.0)).child(select));
    }
    let mut switches = row().w_full().gap(16.0).flex_wrap();
    for (key, label) in PROCESSING_SWITCHES {
        let on = settings.get(key).and_then(|v| v.as_bool()).unwrap_or(false);
        switches = switches.child(
            Checkbox::new(on)
                .label(label)
                .on_toggle(move |s: &mut App| s.set_server_setting(key, serde_json::Value::Bool(!on)))
                .build(k)
                .key(format!("proc-{key}")),
        );
    }
    c = c.child(switches);
    c = c.child(
        col().w_full().gap(4.0).child(text("YouTube caption languages, in order of preference").font(k.ty.small).color(l.text2)).child(
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(
                    col().grow(1.0).min_w(0.0).child(
                        TextInput::new(&p.caption_languages)
                            .width(None)
                            .placeholder("en, es")
                            .label("YouTube caption languages")
                            .on_edit(|s: &mut App| &mut page(s).caption_languages)
                            .on_submit(|s: &mut App, _: &mut Tx| s.save_caption_languages())
                            .build(k)
                            .key(CAPTION_LANGUAGES),
                    ),
                )
                .child(Button::new("Save").on_press(|s: &mut App| s.save_caption_languages()).build(k).key("save-languages")),
        ),
    );
    let mut rebuild = row().gap(10.0).center_y().child(small("Rebuild search index", "rebuild".into(), l, |s| {
        s.ask_to("Rebuild", "Re-embed every source, note and insight on the server? It runs in the background.", |s| s.rebuild_index())
    }));
    if let Some(note) = p.notes.get("rebuild") {
        rebuild = rebuild.child(text(note.clone()).font(k.ty.small).color(l.text2));
    }
    c.child(rebuild)
}
