//! The server admin page: default models, models, API keys and background jobs.

use rune::{El, ScrollSpec, Tx, col, el, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Select, TextInput};

use crate::server::{NewKey, SLOTS, ServerPage};
use crate::state::App;
use crate::theme::{self, Look};

fn page(s: &mut App) -> &mut ServerPage {
    s.server.as_mut().expect("server page")
}

fn section(title: &str, intro: &str, l: &Look) -> El {
    col().w_full().gap(10.0).pt(8.0).child(text(title.to_owned()).font(theme::READ_HEADING).color(l.text)).child(text(intro.to_owned()).font(l.k.ty.ui).color(l.text2).wrap())
}

fn small(label: &str, key: String, l: &Look, f: impl Fn(&mut App) + 'static) -> El {
    Button::new(label.to_owned()).variant(ButtonVariant::Ghost).control_size(ControlSize::Small).on_press(f).build(&l.k).key(key)
}

pub fn view(s: &App, l: &Look, layer: &mut Vec<El>) -> El {
    let k = &l.k;
    let Some(p) = &s.server else { return col() };
    let mut c = col().w_full().max_w(720.0).gap(14.0).px(24.0).py(28.0).child(text("Server").font(theme::READ_TITLE).color(l.text)).child(
        text(format!("Admin for {}.", s.settings.server_url)).font(k.ty.ui).color(l.text2).wrap(),
    );

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
            .build(k, layer)
            .key(format!("slot-{slot}"));
        defaults = defaults.child(row().w_full().gap(12.0).center_y().child(text(label).font(k.ty.ui).color(l.text).w(220.0)).child(select));
    }
    c = c.child(defaults);

    // Models.
    let mut models = section("Models", "Models the server knows about. Sync adds what each provider key can reach.", l).child(row().child(small("Sync models", "sync".into(), l, |s: &mut App| s.sync_models())));
    for m in &p.models {
        let (t, d) = (m.id.clone(), m.id.clone());
        let mut r = row()
            .key(format!("m-{}", m.id))
            .w_full()
            .gap(8.0)
            .py(4.0)
            .center_y()
            .child(col().grow(1.0).min_w(0.0).child(text(m.name.clone()).font(k.ty.ui).color(l.text).max_lines(1)).child(text(format!("{} · {}", m.provider, m.kind)).font(k.ty.small).color(l.text3)));
        if let Some(note) = p.notes.get(&m.id) {
            r = r.child(text(note.clone()).font(k.ty.small).color(l.text2).max_w(220.0).max_lines(2));
        }
        r = r.child(small("Test", "test".into(), l, move |s| s.test_model(t.clone()))).child(small("Remove", "remove".into(), l, move |s| {
            let id = d.clone();
            s.ask("Remove this model from the server?", move |s| s.delete_model(id.clone()));
        }));
        models = models.child(r);
    }
    c = c.child(models);

    // API keys.
    let mut keys = section("API keys", "Provider keys stored on the server, encrypted.", l);
    for cr in &p.credentials {
        let (t, g, d) = (cr.id.clone(), cr.id.clone(), cr.id.clone());
        let detail = match (&cr.decryption_error, cr.model_count) {
            (Some(e), _) => e.clone(),
            (None, Some(n)) => format!("{} · {n} models", cr.provider),
            (None, None) => cr.provider.clone(),
        };
        let mut r = row()
            .key(format!("c-{}", cr.id))
            .w_full()
            .gap(8.0)
            .py(4.0)
            .center_y()
            .child(col().grow(1.0).min_w(0.0).child(text(cr.name.clone()).font(k.ty.ui).color(l.text).max_lines(1)).child(text(detail).font(k.ty.small).color(if cr.decryption_error.is_some() { l.danger } else { l.text3 })));
        if let Some(note) = p.notes.get(&cr.id) {
            r = r.child(text(note.clone()).font(k.ty.small).color(l.text2).max_w(200.0).max_lines(2));
        }
        r = r
            .child(small("Test", "test".into(), l, move |s| s.test_credential(t.clone())))
            .child(small("Add models", "register".into(), l, move |s| s.register_models(g.clone())))
            .child(small("Remove", "remove".into(), l, move |s| {
                let id = d.clone();
                s.ask("Remove this key from the server?", move |s| s.delete_credential(id.clone()));
            }));
        keys = keys.child(r);
    }
    match &p.adding {
        None => keys = keys.child(row().child(small("Add a key", "add-key".into(), l, |s| page(s).adding = Some(NewKey::default())))),
        Some(a) => keys = keys.child(new_key(p, a, l, layer)),
    }
    c = c.child(keys);

    // Jobs and the index.
    let mut jobs = section("Background jobs", "Source processing, embeddings and podcasts running on the server.", l)
        .child(row().gap(8.0).child(small("Refresh", "jobs-refresh".into(), l, |s| s.load_jobs())).child(small("Rebuild search index", "rebuild".into(), l, |s| s.rebuild_index())));
    if let Some(note) = p.notes.get("rebuild") {
        jobs = jobs.child(text(note.clone()).font(k.ty.small).color(l.text2));
    }
    let running: Vec<_> = p.jobs.iter().filter(|j| !matches!(j.status.as_str(), "completed" | "failed" | "canceled" | "cancelled")).collect();
    if running.is_empty() {
        jobs = jobs.child(text("Nothing running").font(k.ty.ui).color(l.text3));
    }
    for j in running {
        let id = j.job_id.clone().or_else(|| j.id.clone()).unwrap_or_default();
        let name = j.name.clone().or_else(|| j.command.clone()).unwrap_or_else(|| "Job".into());
        let cancel = id.clone();
        jobs = jobs.child(
            row()
                .key(format!("j-{id}"))
                .w_full()
                .gap(8.0)
                .center_y()
                .child(text(name).font(k.ty.ui).color(l.text).grow(1.0).min_w(0.0).max_lines(1))
                .child(text(j.status.clone()).font(k.ty.small).color(l.text3))
                .child(small("Cancel", "cancel".into(), l, move |s| s.cancel_job(cancel.clone()))),
        );
    }
    c = c.child(jobs);
    col().key("server").w_full().items_center().child(c).scroll(ScrollSpec { stick_bottom: false }).grow(1.0).min_h(0.0)
}

fn new_key(p: &ServerPage, a: &NewKey, l: &Look, layer: &mut Vec<El>) -> El {
    let k = &l.k;
    fn adding(s: &mut App) -> &mut NewKey {
        page(s).adding.as_mut().expect("adding a key")
    }
    let provider = Select::new("key-provider", &p.providers)
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
        .build(k, layer)
        .key("key-provider");
    col()
        .key("new-key")
        .w_full()
        .gap(8.0)
        .padding(14.0)
        .radius(12.0)
        .border(1.0, l.line_strong)
        .child(provider)
        .child(TextInput::new(&a.name).width(None).placeholder("Name (optional)").on_edit(|s: &mut App| &mut adding(s).name).build(k).key("key-name"))
        .child(TextInput::new(&a.api_key).width(None).secure().placeholder("API key").on_edit(|s: &mut App| &mut adding(s).api_key).build(k).key("key-value"))
        .child(TextInput::new(&a.base_url).width(None).placeholder("Base URL (optional)").on_edit(|s: &mut App| &mut adding(s).base_url).build(k).key("key-url"))
        .child(
            row()
                .w_full()
                .gap(8.0)
                .justify_end()
                .child(el().grow(1.0))
                .child(Button::new("Cancel").variant(ButtonVariant::Ghost).on_press(|s: &mut App| page(s).adding = None).build(k).key("key-cancel"))
                .child(Button::new("Save key").variant(ButtonVariant::Contrast).disabled(a.provider.is_none()).on_press(|s: &mut App| s.add_credential()).build(k).key("key-save")),
        )
}
