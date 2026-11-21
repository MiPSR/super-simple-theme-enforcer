mod config;

use config::{Mode, Paths, Target};
use rune::tok::{self, Theme};
use rune::{AppSpec, Color, Cx, El, ImageCache, SurfaceFit, TextStyle, Tx, col, el, row, text};
use rune_kit::{
    Button, ButtonVariant, ControlSize, Dialog, Divider, IconButton, Kit, PaletteSeed, root,
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
}

impl Default for State {
    fn default() -> Self {
        let (paths, error) = match Paths::discover() {
            Ok(paths) => (Some(paths), None),
            Err(e) => (None, Some(e.to_string())),
        };
        Self {
            paths,
            selected: [true, true, true, true, true, true],
            mode: Mode::Dark,
            feedback: None,
            confirm_remove: false,
            error,
            icon_cache: None,
            portal: None,
            portal_task: Arc::new(Mutex::new(None)),
            wake: None,
        }
    }
}

impl State {
    fn selected(&self) -> impl Iterator<Item = Target> + '_ {
        Target::ALL
            .into_iter()
            .enumerate()
            .filter(|(i, t)| self.selected[*i] && t.supported())
            .map(|(_, t)| t)
    }
    fn can_apply(&self) -> bool {
        self.paths.as_ref().is_some_and(|p| {
            self.selected().any(|t| {
                p.can_apply_mode(t, self.mode) && p.current(t).ok() != Some(Some(self.mode))
            })
        })
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
            if !paths.can_apply_mode(t, mode) {
                continue;
            }
            if paths.current(t).ok() == Some(Some(mode)) {
                continue;
            }
            let result = if defaults && t == Target::Gnome {
                paths.remove(t) // inherit the administrator's dconf preference
            } else {
                paths.apply(t, mode)
            };
            match result {
                Ok(()) => changed += 1,
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
                Ok(()) => changed += 1,
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

fn kit(s: &State) -> Kit {
    let t = theme(s);
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

fn theme(s: &State) -> Theme {
    tok::theme(s.mode == Mode::Dark)
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

fn view(s: &State, _cx: &mut Cx<State>) -> El {
    let t = theme(s);
    let k = kit(s);
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
                                .label("Light theme")
                                .control_size(ControlSize::Regular)
                                .on_press(|s: &mut State| {
                                    s.mode = Mode::Light;
                                    s.feedback = None;
                                })
                                .build(&k)
                                .key("light"),
                        )
                        .child(
                            Button::new("Light")
                                .variant(if s.mode == Mode::Light {
                                    ButtonVariant::Submit
                                } else {
                                    ButtonVariant::default()
                                })
                                .on_press(|s: &mut State| {
                                    s.mode = Mode::Light;
                                    s.feedback = None;
                                })
                                .build(&k),
                        )
                        .child(
                            IconButton::new(&MOON)
                                .label("Dark theme")
                                .control_size(ControlSize::Regular)
                                .on_press(|s: &mut State| {
                                    s.mode = Mode::Dark;
                                    s.feedback = None;
                                })
                                .build(&k)
                                .key("dark"),
                        )
                        .child(
                            Button::new("Dark")
                                .variant(if s.mode == Mode::Dark {
                                    ButtonVariant::Submit
                                } else {
                                    ButtonVariant::default()
                                })
                                .on_press(|s: &mut State| {
                                    s.mode = Mode::Dark;
                                    s.feedback = None;
                                })
                                .build(&k),
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
                .child(
                    text("Apply to")
                        .font(TextStyle::new(14.0, 600.0))
                        .color(t.text),
                )
                .children(
                    Target::ALL
                        .into_iter()
                        .enumerate()
                        .map(|(i, target)| target_row(s, &k, i, target))
                        .collect::<Vec<_>>(),
                )
                .child(
                    text(format!(
                        "Portal preference: {} (read-only). Nix-managed settings are left alone.",
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
                            format!("{}: ColorScheme and color groups only", p.user(t).display())
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
        Dialog::new("remove", "Remove selected user overrides?")
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
                                Button::new("Remove files")
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

fn target_row(s: &State, k: &Kit, i: usize, target: Target) -> El {
    let supported = target.supported();
    let paths = s.paths.as_ref();
    let writable = paths.is_some_and(|p| p.writable(target));
    let detail = if !supported {
        "Set QT_QPA_PLATFORMTHEME=qt6ct to enable"
    } else if !writable {
        "Nix-managed / locked"
    } else if !paths.is_some_and(|p| p.can_apply_mode(target, s.mode)) {
        "No installed scheme for this mode"
    } else if paths.is_some_and(|p| p.exists(target)) {
        "User file present"
    } else {
        "No user file"
    };
    row()
        .key(format!("target-{i}"))
        .w_full()
        .h(53.0)
        .gap(12.0)
        .center_y()
        .child(
            col()
                .grow(1.0)
                .gap(3.0)
                .child(
                    text(target.name())
                        .font(TextStyle::new(14.0, 600.0))
                        .color(k.c.text_base),
                )
                .child(text(detail).font(k.ty.small).color(k.c.text_muted)),
        )
        .child(
            Button::new(if s.selected[i] { "Selected" } else { "Select" })
                .variant(if s.selected[i] {
                    ButtonVariant::Submit
                } else {
                    ButtonVariant::default()
                })
                .disabled(
                    !supported
                        || !writable
                        || !paths.is_some_and(|p| {
                            p.exists(target)
                                || p.can_apply_mode(target, s.mode)
                                || p.default(target).ok().flatten().is_some()
                        }),
                )
                .on_press(move |s: &mut State| {
                    s.selected[i] = !s.selected[i];
                    s.feedback = None;
                })
                .build(k),
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
                "Removed"
            } else {
                "Remove"
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
