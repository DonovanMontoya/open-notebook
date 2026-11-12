//! Touch choices use a bounded sheet; the desktop keeps the kit's inline listbox.

use std::{cell::RefCell, rc::Rc};

use rune::{El, Tx, col, el, icon, row, text};
use rune_kit::Kit;

use crate::state::App;

type Action = Rc<dyn Fn(&mut App)>;
type Choose = Rc<dyn Fn(&mut App, &str, &mut Tx)>;

thread_local! {
    static SHEETS: RefCell<Vec<El>> = const { RefCell::new(Vec::new()) };
}

/// A view can build a picker while it is still assembling its containing dialog.
pub fn begin_layers() {
    SHEETS.with(|sheets| sheets.borrow_mut().clear());
}

/// Mount pickers after their containing dialogs so modal focus and input follow paint order.
pub fn finish_layers(layer: &mut Vec<El>) {
    SHEETS.with(|sheets| layer.append(&mut sheets.borrow_mut()));
}

pub struct Select {
    inner: rune_kit::Select,
    key: String,
    options: Vec<(Option<String>, String)>,
    details: Vec<Option<String>>,
    current: Option<String>,
    placeholder: String,
    label: Option<String>,
    open: bool,
    disabled: bool,
    invalid: bool,
    width: f32,
    toggle: Option<Action>,
    close: Option<Action>,
    choose: Option<Choose>,
}

impl Select {
    pub fn new<S: AsRef<str>>(key: impl Into<String>, options: &[S]) -> Self {
        let key = key.into();
        Self {
            inner: rune_kit::Select::new(key.clone(), options),
            key,
            options: options.iter().map(|s| (None, s.as_ref().to_owned())).collect(),
            details: Vec::new(),
            current: None,
            placeholder: String::new(),
            label: None,
            open: false,
            disabled: false,
            invalid: false,
            width: 280.0,
            toggle: None,
            close: None,
            choose: None,
        }
    }

    pub fn details<S: AsRef<str>>(mut self, details: &[Option<S>]) -> Self {
        self.inner = self.inner.details(details);
        self.details = details.iter().map(|d| d.as_ref().map(|s| s.as_ref().to_owned())).collect();
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.inner = self.inner.width(width);
        self.width = width;
        self
    }

    pub fn current(mut self, current: Option<&str>) -> Self {
        self.inner = self.inner.current(current);
        self.current = current.map(str::to_owned);
        self
    }

    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        let placeholder = placeholder.into();
        self.inner = self.inner.placeholder(placeholder.clone());
        self.placeholder = placeholder;
        self
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        let label = label.into();
        self.inner = self.inner.label(label.clone());
        self.label = Some(label);
        self
    }

    pub fn open(mut self, open: bool) -> Self {
        self.inner = self.inner.open(open);
        self.open = open;
        self
    }

    pub fn on_toggle(mut self, f: impl Fn(&mut App) + 'static) -> Self {
        let f: Action = Rc::new(f);
        let desktop = f.clone();
        self.inner = self.inner.on_toggle(move |s: &mut App| desktop(s));
        self.toggle = Some(f);
        self
    }

    pub fn on_close(mut self, f: impl Fn(&mut App) + 'static) -> Self {
        let f: Action = Rc::new(f);
        let desktop = f.clone();
        self.inner = self.inner.on_close(move |s: &mut App| desktop(s));
        self.close = Some(f);
        self
    }

    pub fn on_choose(mut self, f: impl Fn(&mut App, &str, &mut Tx) + 'static) -> Self {
        let f: Choose = Rc::new(f);
        let desktop = f.clone();
        self.inner = self.inner.on_choose(move |s: &mut App, value: &str, tx: &mut Tx| desktop(s, value, tx));
        self.choose = Some(f);
        self
    }

    pub fn build(self, k: &Kit, layer: &mut Vec<El>) -> El {
        if k.ty.ui.size < 16.0 {
            return self.inner.build(k, layer);
        }
        let trigger = self.touch_trigger(k);
        if self.open && !self.disabled {
            self.sheet(k);
        }
        trigger
    }

    fn touch_trigger(&self, k: &Kit) -> El {
        let label = self.current.as_ref().unwrap_or(&self.placeholder);
        let mut trigger = row()
            .key(format!("{}-trigger", self.key))
            .role("combobox")
            .label(self.label.as_ref().unwrap_or(&self.placeholder).clone())
            .a11y_value(label.clone())
            .expanded(self.open)
            .disabled(self.disabled)
            .focusable(!self.disabled)
            .focus_ring(1.0, 0.0, k.c.border_focus)
            .w(self.width)
            .max_w(self.width)
            .min_w(0.0)
            .min_h(44.0)
            .px(12.0)
            .py(8.0)
            .gap(8.0)
            .center_y()
            .radius(8.0)
            .fill(k.c.bg_base)
            .border(1.0, if self.invalid { k.c.danger.fg } else { k.c.border_base })
            .child(text(label.clone()).font(k.ty.ui.lh(22.0)).color(k.c.text_base).wrap().max_lines(2).grow(1.0).shrink().min_w(0.0))
            .child(icon(k.glyph.chevron_down, 16.0).color(k.c.icon_muted).a11y_hidden());
        if self.disabled {
            trigger = trigger.dim(0.5);
        } else if let Some(toggle) = &self.toggle {
            let toggle = toggle.clone();
            trigger = k.interactive(trigger).on_press_tx(move |s: &mut App, tx: &mut Tx| {
                tx.blur();
                toggle(s);
            });
        }
        trigger
    }

    fn sheet(&self, k: &Kit) {
        let Some(close) = self.close.as_ref().or(self.toggle.as_ref()).cloned() else { return };
        let title = self.label.as_ref().unwrap_or(&self.placeholder);
        let done = close.clone();
        let done = k
            .interactive(
                row()
                    .key("done")
                    .role("button")
                    .label("Done")
                    .h(44.0)
                    .min_w(64.0)
                    .px(12.0)
                    .center()
                    .child(text("Done").font(k.ty.ui_medium).color(k.c.text_base)),
            )
            .on_press(move |s: &mut App| done(s));
        let header = row()
            .w_full()
            .min_h(60.0)
            .pl(20.0)
            .pr(8.0)
            .gap(12.0)
            .center_y()
            .child(text(title.clone()).font(k.ty.ui_medium.lh(24.0)).color(k.c.text_base).wrap().grow(1.0).min_w(0.0))
            .child(done);
        let mut list = col().key("options").w_full().gap(0.0).pb(12.0);
        let mut group = None;
        for (i, (g, value)) in self.options.iter().enumerate() {
            if g.as_deref() != group {
                group = g.as_deref();
                if let Some(group) = group {
                    list = list.child(text(group.to_owned()).font(k.ty.ui_medium).color(k.c.text_muted).px(20.0).pt(20.0).pb(8.0));
                }
            }
            let selected = self.current.as_ref() == Some(value);
            let mut lines = col().grow(1.0).shrink().min_w(0.0).gap(4.0).child(text(value.clone()).font(k.ty.ui.lh(24.0)).color(k.c.text_base).wrap());
            if let Some(Some(detail)) = self.details.get(i) {
                lines = lines.child(text(detail.clone()).font(k.ty.small.lh(20.0)).color(k.c.text_muted).wrap());
            }
            let mut option = row()
                .key(format!("option{i}"))
                .role("option")
                .label(value.clone())
                .focusable(true)
                .focus_selected(selected)
                .position_in_set(i + 1, self.options.len())
                .w_full()
                .min_h(52.0)
                .px(20.0)
                .py(12.0)
                .gap(12.0)
                .center_y()
                .child(lines)
                .child(el().size(20.0, 20.0).center().child(icon(k.glyph.check_small, 18.0).color(k.c.icon_base).opacity(if selected { 1.0 } else { 0.0 })));
            if let Some(choose) = &self.choose {
                let choose = choose.clone();
                let value = value.clone();
                let close = close.clone();
                option = k.interactive(option).on_press_tx(move |s: &mut App, tx: &mut Tx| {
                    choose(s, &value, tx);
                    close(s);
                });
            }
            list = list.child(option);
        }
        let panel = col()
            .key(format!("{}-sheet", self.key))
            .role("dialog")
            .label(title.clone())
            .focus_return(format!("{}-trigger", self.key))
            .w_full()
            .max_w(560.0)
            .h_full()
            .max_h(520.0)
            .min_h(0.0)
            .fill(k.c.layer_01)
            .radius(16.0)
            .clip_rounded(16.0)
            .child(header)
            .child(el().w_full().h(1.0).fill(k.c.border_base))
            .child(
                col()
                    .key("listbox")
                    .role("listbox")
                    .label(title.clone())
                    .focus_group()
                    .w_full()
                    .grow(1.0)
                    .min_h(0.0)
                    .child(list.scroll(rune::ScrollSpec { stick_bottom: false }).w_full().h_full()),
            )
            .modal(true, move |s: &mut App| close(s));
        SHEETS.with(|sheets| {
            let mut sheets = sheets.borrow_mut();
            sheets.push(el().key(format!("{}-scrim", self.key)).absolute(0.0, 0.0).w_full().h_full().fill(k.c.scrim).a11y_hidden().raise());
            sheets.push(
                col().key(format!("{}-layer", self.key)).absolute(0.0, 0.0).w_full().h_full().padding(16.0).justify_end().items_center().child(panel).raise(),
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{Cx, Env, Runtime};

    fn nested_picker(_: &App, _: &mut Cx<App>) -> El {
        begin_layers();
        let look = crate::theme::Look::new(false).sized(393.0);
        let mut layer = Vec::new();
        let trigger = Select::new("test-select", &["A long research model name that needs more than a single line on a phone", "Other model"])
            .label("Research model")
            .open(true)
            .on_close(|_: &mut App| {})
            .on_choose(|s: &mut App, value: &str, _: &mut Tx| s.settings.appearance = value.to_owned())
            .build(&look.k, &mut layer);
        // The containing dialog is assembled after its child select, as in the profile editor.
        layer.push(col().key("parent-modal").absolute(0.0, 0.0).w_full().h_full().fill(look.panel).modal(true, |_: &mut App| {}).child(trigger));
        finish_layers(&mut layer);
        rune_kit::root(col().key("root").w_full().h_full(), layer)
    }

    #[test]
    fn picker_remains_touchable_above_its_containing_dialog() {
        let mut runtime = Runtime::new(App::offline(), nested_picker, (393.0, 678.0), Env::new(false, 1.0));
        runtime.frame(0.0);
        let [x, y, w, h] = runtime.presented("option0").expect("first option").0;
        assert!(h >= 52.0 && x >= 0.0 && x + w <= 393.0 && y + h <= 678.0, "option bounds: {x} {y} {w} {h}");
        assert!(runtime.presented("done").expect("Done action").0[3] >= 44.0);
        runtime.pointer_down(x + w / 2.0, y + h / 2.0);
        runtime.pointer_up(x + w / 2.0, y + h / 2.0);
        runtime.frame(0.1);
        assert!(runtime.state.settings.appearance.starts_with("A long research model"), "the parent modal must not block the choice");
    }
}
