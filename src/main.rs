mod config;

use config::{Catalog, Mode, Paths, Selection, Target};
use rune::tok;
use rune::{AppSpec, Color, Cx, El, ImageCache, SurfaceFit, TextStyle, Tx, col, el, row, text};
use rune_kit::{
    Button, ButtonVariant, Checkbox, ControlSize, Dialog, Divider, IconButton, Kit, PaletteSeed,
    Select, SelectAppearance, root,
};
use std::sync::{Arc, Mutex};

static SUN: rune::Icon = rune::Icon {
    name: "sste-sun",
    view: 24.0,
    body: r#"<circle cx="12" cy="12" r="4" fill="none" stroke="currentColor" stroke-width="2"/><path d="M12 2v2m0 16v2M2 12h2m16 0h2M4.9 4.9l1.4 1.4m11.4 11.4 1.4 1.4M19.1 4.9l-1.4 1.4M6.3 17.7l-1.4 1.4" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"/>"#,
};
static MOON: rune::Icon = rune::Icon {
    name: "sste-moon",
    view: 24.0,
    body: r#"<path d="M20.2 15.4A8.6 8.6 0 0 1 8.6 3.8 8.7 8.7 0 1 0 20.2 15.4Z" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"/>"#,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Feedback {
    Applied,
    Removed,
    Defaults,
}

struct State {
    paths: Option<Paths>,
    selected: [bool; 6],
    mode: Mode,
    feedback: Option<Feedback>,
    confirm_remove: bool,
    error: Option<String>,
    icon_cache: Option<Arc<ImageCache>>,
    portal: Option<Mode>,
    portal_task: Arc<Mutex<Option<Option<Mode>>>>,
    wake: Option<rune::Waker>,
    catalog: Catalog,
    unavailable: [Option<String>; 6],
    themes: [[Option<String>; 6]; 2],
    styles: [Option<String>; 2],
    open_select: Option<usize>,
}

impl Default for State {
    fn default() -> Self {
        let (paths, error) = match Paths::discover() {
            Ok(paths) => (Some(paths), None),
            Err(e) => (None, Some(e.to_string())),
        };
        let catalog = Catalog::discover(paths.as_ref());
        let unavailable = std::array::from_fn(|i| {
            paths
                .as_ref()
                .map(|p| p.unavailable(Target::ALL[i]))
                .unwrap_or_else(|| Some("Configuration directory unavailable".into()))
        });
        let selected = std::array::from_fn(|i| unavailable[i].is_none());
        Self {
            paths,
            selected,
            mode: Mode::Dark,
            feedback: None,
            confirm_remove: false,
            error,
            icon_cache: None,
            portal: None,
            portal_task: Arc::new(Mutex::new(None)),
            wake: None,
            catalog,
            unavailable,
            themes: std::array::from_fn(|_| std::array::from_fn(|_| None)),
            styles: [None, None],
            open_select: None,
        }
    }
}

impl State {
    fn mode_index(&self) -> usize {
        usize::from(self.mode == Mode::Dark)
    }
    fn selection(&self, target: Target) -> Selection<'_> {
        let i = Target::ALL.iter().position(|t| *t == target).unwrap();
        let theme = self.themes[self.mode_index()][i].as_ref().and_then(|name| {
            self.catalog
                .choices(target)
                .iter()
                .find(|t| &t.name == name)
        });
        Selection {
            theme,
            style: self.styles[self.mode_index()].as_deref(),
        }
    }
    fn ready(&self, target: Target, mode: Mode) -> bool {
        self.paths.as_ref().is_some_and(|p| {
            p.writable(target)
                && (self.selection(target).theme.is_some() || p.can_apply_mode(target, mode))
        })
    }
    fn selected(&self) -> impl Iterator<Item = Target> + '_ {
        Target::ALL
            .into_iter()
            .enumerate()
            .filter(|(i, _)| self.selected[*i] && self.unavailable[*i].is_none())
            .map(|(_, t)| t)
    }
    fn can_apply(&self) -> bool {
        self.selected().any(|t| self.ready(t, self.mode))
    }
    fn can_remove(&self) -> bool {
        self.paths
            .as_ref()
            .is_some_and(|p| self.selected().any(|t| p.writable(t) && p.exists(t)))
    }
    fn can_defaults(&self) -> bool {
        self.paths.as_ref().is_some_and(|p| {
            self.selected().any(|t| {
                p.default(t).ok().flatten().is_some_and(|default| {
                    p.can_apply_mode(t, default) && p.current(t).ok() != Some(Some(default))
                })
            })
        })
    }
    fn apply(&mut self, defaults: bool) {
        self.feedback = None;
        let Some(paths) = self.paths.as_ref() else {
            return;
        };
        let targets: Vec<_> = self.selected().collect();
        let mut errors = Vec::new();
        let mut changed = 0;
        for t in targets {
            let mode = if defaults {
                paths.default(t).ok().flatten()
            } else {
                Some(self.mode)
            };
            let Some(mode) = mode else { continue };
            if !(if defaults {
                paths.can_apply_mode(t, mode)
            } else {
                self.ready(t, mode)
            }) {
                errors.push(format!(
                    "{}: no installed matching theme; choose one",
                    t.name()
                ));
                continue;
            }
            let result = if defaults && t == Target::Gnome {
                paths.remove(t) // inherit the administrator's dconf preference
            } else {
                if defaults {
                    paths.apply(t, mode)
                } else {
                    paths.apply_selected(t, mode, self.selection(t))
                }
            };
            match result {
                Ok(()) => {
                    changed += 1;
                    if t == Target::Plasma {
                        config::notify_kde();
                    }
                }
                Err(e) => errors.push(format!("{}: {e}", t.name())),
            }
        }
        if !errors.is_empty() {
            self.error = Some(errors.join("\n"));
        } else if changed > 0 {
            self.feedback = Some(if defaults {
                Feedback::Defaults
            } else {
                Feedback::Applied
            });
            self.refresh_portal();
        }
    }
    fn remove(&mut self) {
        self.confirm_remove = false;
        self.feedback = None;
        let Some(paths) = self.paths.as_ref() else {
            return;
        };
        let targets: Vec<_> = self.selected().collect();
        let mut errors = Vec::new();
        let mut changed = 0;
        for t in targets {
            if !paths.writable(t) || !paths.exists(t) {
                continue;
            }
            match paths.remove(t) {
                Ok(()) => {
                    changed += 1;
                    if t == Target::Plasma {
                        config::notify_kde();
                    }
                }
                Err(e) => errors.push(format!("{}: {e}", t.name())),
            }
        }
        if !errors.is_empty() {
            self.error = Some(errors.join("\n"));
        } else if changed > 0 {
            self.feedback = Some(Feedback::Removed);
            self.refresh_portal();
        }
    }

    fn refresh_portal(&self) {
        let Some(waker) = self.wake.clone() else {
            return;
        };
        let task = self.portal_task.clone();
        std::thread::spawn(move || {
            *task.lock().unwrap_or_else(|e| e.into_inner()) = Some(config::portal_preference());
            waker();
        });
    }
}

fn setup(s: &mut State, waker: rune::Waker) {
    s.icon_cache = Some(Arc::new(ImageCache::new(waker.clone())));
    s.wake = Some(waker);
    s.refresh_portal();
}

fn poll(s: &mut State, _now: f64, _tx: &mut Tx) -> bool {
    if let Some(value) = s
        .portal_task
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
    {
        s.portal = value;
        true
    } else {
        false
    }
}

fn kit(dark: bool) -> Kit {
    let t = tok::theme(dark);
    let accent = rune::oklch(0.543, 0.093, 194.77);
    let mut k = Kit::from_palette(PaletteSeed {
        bg: t.bg,
        fg: t.text,
        accent,
    });
    k.c.bg_icon_button_contrast = accent;
    k.c.submit_fg = Color::WHITE;
    k.c.submit_icon = Color::WHITE;
    k.c.submit_hover = Color::WHITE.alpha(0.12);
    k.c.contrast_pressed = Color::WHITE.alpha(0.20);
    k
}

fn logo(s: &State) -> El {
    let Some(cache) = &s.icon_cache else {
        return el().size(30.0, 30.0);
    };
    let image = cache.svg(
        include_bytes!("../assets/super-simple-theme-enforcer.svg").as_slice(),
        30.0,
        2.0,
    );
    el().key("logo")
        .size(30.0, 30.0)
        .pixels_fit(image.pixels(), SurfaceFit::Fit)
}

fn view(s: &State, cx: &mut Cx<State>) -> El {
    let t = tok::theme(cx.system_dark());
    let k = kit(cx.system_dark());
    let mut layer = Vec::new();
    let content = col()
        .key("app")
        .w_full()
        .h_full()
        .fill(t.bg)
        .child(
            row()
                .key("header")
                .h(60.0)
                .w_full()
                .px(18.0)
                .gap(9.0)
                .center_y()
                .child(logo(s))
                .child(
                    text("Super Simple Theme Enforcer")
                        .font(TextStyle::new(17.0, 600.0))
                        .color(t.text),
                ),
        )
        .child(
            col()
                .w_full()
                .px(22.0)
                .pt(12.0)
                .pb(16.0)
                .gap(12.0)
                .child(
                    text("Appearance")
                        .font(TextStyle::new(14.0, 600.0))
                        .color(t.text),
                )
                .child(
                    row()
                        .key("appearance")
                        .center_y()
                        .gap(12.0)
                        .child(
                            IconButton::new(&SUN)
                                .label(if s.mode == Mode::Light {
                                    "Light (pending)"
                                } else {
                                    "Choose light"
                                })
                                .control_size(ControlSize::Regular)
                                .on_press(|s: &mut State| {
                                    s.mode = Mode::Light;
                                    s.feedback = None;
                                    s.open_select = None;
                                })
                                .build(&k)
                                .key("light"),
                        )
                        .child(
                            IconButton::new(&MOON)
                                .label(if s.mode == Mode::Dark {
                                    "Dark (pending)"
                                } else {
                                    "Choose dark"
                                })
                                .control_size(ControlSize::Regular)
                                .on_press(|s: &mut State| {
                                    s.mode = Mode::Dark;
                                    s.feedback = None;
                                    s.open_select = None;
                                })
                                .build(&k)
                                .key("dark"),
                        )
                        .child(
                            text(format!("{} — press Apply to save", s.mode.label()))
                                .font(k.ty.small)
                                .color(k.c.text_muted),
                        ),
                ),
        )
        .child(Divider::horizontal().build(&k))
        .child(
            col()
                .key("targets")
                .w_full()
                .grow(1.0)
                .px(22.0)
                .pt(12.0)
                .gap(2.0)
                .min_h(0.0)
                .scroll(rune::ScrollSpec {
                    stick_bottom: false,
                })
                .child(
                    text("Apply to")
                        .font(TextStyle::new(14.0, 600.0))
                        .color(t.text),
                )
                .children(
                    Target::ALL
                        .into_iter()
                        .enumerate()
                        .map(|(i, target)| target_row(s, &k, i, target, &mut layer))
                        .collect::<Vec<_>>(),
                )
                .child(style_row(s, &k, &mut layer))
                .child(
                    text(format!(
                        "Last portal read: {} (read-only). Window appearance follows the system. Nix-managed settings are left alone.",
                        s.portal.map(Mode::label).unwrap_or("No preference")
                    ))
                    .wrap()
                    .font(k.ty.small)
                    .color(k.c.text_muted),
                ),
        )
        .child(Divider::horizontal().build(&k))
        .child(footer(s, &k));
    if s.confirm_remove {
        let files = s
            .paths
            .as_ref()
            .map(|p| {
                s.selected()
                    .filter(|t| p.exists(*t))
                    .map(|t| match t {
                        Target::Gnome => {
                            "GSettings: org.gnome.desktop.interface/color-scheme (reset)".into()
                        }
                        Target::Xfce => "xfconf: xsettings /Net/ThemeName (reset)".into(),
                        Target::Plasma => {
                            format!("{}: ColorScheme (General / UiSettings), color groups and widgetStyle only", p.user(t).display())
                        }
                        Target::Qt6 => format!(
                            "{}\n{}",
                            p.user(t).display(),
                            p.user(t)
                                .parent()
                                .unwrap()
                                .join("colors/super-simple-theme-enforcer.conf")
                                .display()
                        ),
                        _ => p.user(t).display().to_string(),
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        Dialog::new("remove", "Delete selected user overrides?")
            .description("Deletes listed user theme files or resets the specified settings.")
            .fit()
            .open(true)
            .on_close(|s: &mut State| s.confirm_remove = false)
            .child(
                col()
                    .px(16.0)
                    .pb(16.0)
                    .gap(14.0)
                    .child(text(files).wrap().font(k.ty.small))
                    .child(
                        row()
                            .justify_end()
                            .gap(8.0)
                            .child(
                                Button::new("Cancel")
                                    .on_press(|s: &mut State| s.confirm_remove = false)
                                    .build(&k),
                            )
                            .child(
                                Button::new("Delete overrides")
                                    .variant(ButtonVariant::Submit)
                                    .on_press(|s: &mut State| s.remove())
                                    .build(&k),
                            ),
                    ),
            )
            .build(&k, &mut layer);
    }
    if let Some(error) = &s.error {
        Dialog::new("error", "Operation needs attention")
            .fit()
            .open(true)
            .on_close(|s: &mut State| s.error = None)
            .child(
                col()
                    .px(16.0)
                    .pb(16.0)
                    .gap(12.0)
                    .child(text(error.clone()).wrap())
                    .child(
                        row().justify_end().child(
                            Button::new("OK")
                                .on_press(|s: &mut State| s.error = None)
                                .build(&k),
                        ),
                    ),
            )
            .build(&k, &mut layer);
    }
    root(content, layer)
}

fn target_row(s: &State, k: &Kit, i: usize, target: Target, layer: &mut Vec<El>) -> El {
    let paths = s.paths.as_ref();
    let detail = if target == Target::Qt6 {
        "Writes qt6ct configuration; only apps using qt6ct read it"
    } else if target == Target::Plasma {
        "kdeglobals: KDE widgets and Kirigami, inside or outside Plasma"
    } else if !s.ready(target, s.mode) && s.unavailable[i].is_none() {
        "Choose an installed theme for this mode"
    } else if paths.is_some_and(|p| p.exists(target)) {
        "User file present"
    } else {
        "No user file"
    };
    let mut content = col()
        .key(format!("target-{i}"))
        .w_full()
        .py(8.0)
        .gap(5.0)
        .child(
            row()
                .w_full()
                .center_y()
                .gap(10.0)
                .child(
                    Checkbox::new(s.selected[i])
                        .label(target.name())
                        .disabled(s.unavailable[i].is_some() && !s.selected[i])
                        .on_toggle(move |s: &mut State| {
                            s.selected[i] = !s.selected[i];
                            s.feedback = None;
                            s.open_select = None;
                        })
                        .build(k),
                )
                .child(el().grow(1.0))
                .children(
                    s.unavailable[i]
                        .as_ref()
                        .map(|reason| Button::new(reason).disabled(true).build(k))
                        .into_iter()
                        .collect::<Vec<_>>(),
                ),
        );
    content = content.child(text(detail).font(k.ty.small).color(k.c.text_muted));
    if target != Target::Gnome && s.selected[i] {
        let choices = s.catalog.choices(target);
        let automatic = match target {
            Target::Plasma => "Breeze (matching mode)",
            Target::Qt6 => "Generated palette (matching mode)",
            Target::Xfce => "Match current theme family",
            _ => "Keep current theme",
        };
        let options: Vec<_> = std::iter::once(automatic.to_owned())
            .chain(choices.iter().map(|t| t.name.clone()))
            .collect();
        content = content.child(
            Select::new(format!("theme-{i}"), &options)
                .label(format!("{} theme for {}", target.name(), s.mode.label()))
                .appearance(SelectAppearance::Field)
                .current(Some(
                    s.themes[s.mode_index()][i]
                        .as_deref()
                        .unwrap_or(&options[0]),
                ))
                .disabled(!s.selected[i] || s.unavailable[i].is_some() || choices.is_empty())
                .open(s.open_select == Some(i))
                .on_toggle(move |s: &mut State| {
                    s.open_select = if s.open_select == Some(i) {
                        None
                    } else {
                        Some(i)
                    }
                })
                .on_close(|s: &mut State| s.open_select = None)
                .on_choose(move |s: &mut State, value, _| {
                    let m = s.mode_index();
                    s.themes[m][i] = (value != automatic).then(|| value.to_owned());
                    s.open_select = None;
                    s.feedback = None;
                })
                .build(k, layer),
        );
        if choices.is_empty() {
            content = content.child(
                text("No installed themes discovered")
                    .font(k.ty.small)
                    .color(k.c.text_muted),
            );
        }
    }
    content
}

fn style_row(s: &State, k: &Kit, layer: &mut Vec<El>) -> El {
    let enabled = s
        .selected()
        .any(|t| matches!(t, Target::Qt6 | Target::Plasma));
    if !enabled {
        return el().key("no-qt-style");
    }
    let options: Vec<_> = std::iter::once("Keep existing style".to_owned())
        .chain(s.catalog.styles.iter().cloned())
        .collect();
    col()
        .w_full()
        .gap(5.0)
        .py(8.0)
        .child(text("Qt widget style").font(k.ty.small))
        .child(
            Select::new("qt-style", &options)
                .label("Qt widget style")
                .appearance(SelectAppearance::Field)
                .current(Some(
                    s.styles[s.mode_index()].as_deref().unwrap_or(&options[0]),
                ))
                .disabled(!enabled || s.catalog.styles.is_empty())
                .open(enabled && s.open_select == Some(6))
                .on_toggle(|s: &mut State| {
                    s.open_select = if s.open_select == Some(6) {
                        None
                    } else {
                        Some(6)
                    }
                })
                .on_close(|s: &mut State| s.open_select = None)
                .on_choose(|s: &mut State, value, _| {
                    let m = s.mode_index();
                    s.styles[m] = (value != "Keep existing style").then(|| value.to_owned());
                    s.open_select = None;
                    s.feedback = None;
                })
                .build(k, layer),
        )
        .children(
            (enabled && s.catalog.styles.is_empty())
                .then(|| {
                    Button::new("Qt style discovery unavailable (sste-qt-styles)")
                        .disabled(true)
                        .build(k)
                })
                .into_iter()
                .collect::<Vec<_>>(),
        )
}

fn footer(s: &State, k: &Kit) -> El {
    row()
        .key("footer")
        .h(64.0)
        .w_full()
        .px(16.0)
        .gap(8.0)
        .center_y()
        .child(
            Button::new(if s.feedback == Some(Feedback::Defaults) {
                "Restored"
            } else {
                "Defaults"
            })
            .disabled(!s.can_defaults())
            .on_press(|s: &mut State| s.apply(true))
            .build(k)
            .key("defaults"),
        )
        .child(
            Button::new(if s.feedback == Some(Feedback::Removed) {
                "Deleted"
            } else {
                "Delete overrides"
            })
            .disabled(!s.can_remove())
            .on_press(|s: &mut State| {
                s.feedback = None;
                s.confirm_remove = true;
            })
            .build(k)
            .key("remove"),
        )
        .child(
            Button::new(if s.feedback == Some(Feedback::Applied) {
                "Applied"
            } else {
                "Apply"
            })
            .disabled(!s.can_apply())
            .on_press(|s: &mut State| s.apply(false))
            .build(k)
            .key("apply"),
        )
        .child(el().grow(1.0))
        .child(
            Button::new("OK")
                .variant(ButtonVariant::Submit)
                .on_press_tx(|_s: &mut State, tx: &mut Tx| tx.close_window())
                .build(k)
                .key("ok"),
        )
        .child(
            Button::new("Close")
                .on_press_tx(|_s: &mut State, tx: &mut Tx| tx.close_window())
                .build(k)
                .key("close"),
        )
}

fn main() {
    let nixos = std::fs::read_to_string("/etc/os-release")
        .is_ok_and(|s| s.lines().any(|l| l == "ID=nixos"));
    if !nixos || !config::wayland() {
        eprintln!("Super Simple Theme Enforcer requires NixOS and a Wayland session.");
        std::process::exit(1);
    }
    rune::set_app_id("io.github.MiPSR.SuperSimpleThemeEnforcer");
    rune::run(AppSpec::<State> {
        min_size: Some((640.0, 600.0)),
        setup: Some(setup),
        poll: Some(poll),
        ..AppSpec::new(
            "Super Simple Theme Enforcer",
            (760.0, 650.0),
            State::default,
            view,
        )
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime(system_dark: bool) -> rune::Runtime<State> {
        // No discovery, commands, session bus, window or writes in UI tests.
        let state = State {
            paths: None,
            selected: [false; 6],
            mode: Mode::Dark,
            feedback: None,
            confirm_remove: false,
            error: None,
            icon_cache: None,
            portal: None,
            portal_task: Arc::new(Mutex::new(None)),
            wake: None,
            catalog: Catalog::default(),
            unavailable: std::array::from_fn(|_| Some("Unavailable in test".into())),
            themes: std::array::from_fn(|_| std::array::from_fn(|_| None)),
            styles: [None, None],
            open_select: None,
        };
        let mut rt = rune::Runtime::new(
            state,
            view,
            (760.0, 650.0),
            rune::Env {
                system_dark,
                scale: 1.0,
            },
        );
        rt.frame(0.0);
        rt
    }

    #[test]
    fn pending_mode_never_recolors_the_app() {
        let mut rt = runtime(true);
        let before = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("app"))
            .unwrap()
            .fill;
        let (x, y) = rt.find("light").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
        assert_eq!(rt.state.mode, Mode::Light);
        let after = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("app"))
            .unwrap()
            .fill;
        assert_eq!(before, after);
        assert_eq!(after, tok::theme(true).bg);
        let light = runtime(false);
        let actual = light
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("app"))
            .unwrap()
            .fill;
        assert_eq!(actual, tok::theme(false).bg);
        assert_eq!(light.state.mode, Mode::Dark);
    }

    #[test]
    fn unavailable_targets_are_unchecked_and_explain_why() {
        let rt = runtime(true);
        let elements = rt.elements();
        assert_eq!(elements.iter().filter(|e| e.role == "checkbox").count(), 6);
        assert_eq!(
            elements
                .iter()
                .filter(|e| e.role == "button" && e.label.as_deref() == Some("Unavailable in test"))
                .count(),
            6
        );
        assert!(rt.state.selected.iter().all(|s| !s));
        assert!(
            !elements
                .iter()
                .any(|e| e.key.as_deref() == Some("qt-style-trigger"))
        );
    }

    #[test]
    fn a_selected_target_can_be_unchecked_even_if_it_becomes_unavailable() {
        let mut rt = runtime(true);
        rt.state.selected[0] = true;
        rt.invalidate();
        rt.frame(1.0 / 60.0);
        let checkbox = rt
            .elements()
            .into_iter()
            .find(|e| e.role == "checkbox" && e.label.as_deref() == Some(Target::Gnome.name()))
            .unwrap();
        assert!(checkbox.focusable);
        let [x, y, w, h] = checkbox.rect;
        rt.pointer_down(x + w / 2.0, y + h / 2.0);
        rt.pointer_up(x + w / 2.0, y + h / 2.0);
        rt.frame(2.0 / 60.0);
        assert!(!rt.state.selected[0]);
    }

    #[test]
    fn theme_and_style_dropdowns_require_the_matching_checked_target() {
        let mut rt = runtime(true);
        rt.state.unavailable = std::array::from_fn(|_| None);
        rt.state.catalog.styles = vec!["Fusion".into()];
        rt.state.catalog.qt = vec![config::ThemeChoice {
            name: "Test".into(),
            path: "/unused/test.conf".into(),
        }];
        rt.state.catalog.gtk3 = vec![config::ThemeChoice {
            name: "Test".into(),
            path: "/unused/gtk".into(),
        }];
        let focusable = |rt: &rune::Runtime<State>, key: &str| {
            rt.elements()
                .into_iter()
                .find(|e| e.key.as_deref() == Some(key))
                .is_some_and(|e| e.focusable)
        };
        rt.state.selected[1] = true;
        rt.invalidate();
        rt.frame(1.0 / 60.0);
        assert!(focusable(&rt, "theme-1-trigger"));
        assert!(!focusable(&rt, "theme-4-trigger"));
        assert!(!focusable(&rt, "qt-style-trigger"));
        rt.state.selected[4] = true;
        rt.invalidate();
        rt.frame(2.0 / 60.0);
        assert!(focusable(&rt, "theme-4-trigger"));
        assert!(focusable(&rt, "qt-style-trigger"));
        rt.state.selected[4] = false;
        rt.invalidate();
        rt.frame(3.0 / 60.0);
        assert!(!focusable(&rt, "qt-style-trigger"));
    }
}
