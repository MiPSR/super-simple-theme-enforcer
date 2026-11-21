//! Only fixed, user-side paths are writable. No directory-wide deletion.
use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::Command;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Light,
    Dark,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Gnome,
    Gtk3,
    Gtk4,
    Plasma,
    Qt6,
    Xfce,
}

impl Target {
    pub const ALL: [Self; 6] = [
        Self::Gnome,
        Self::Gtk3,
        Self::Gtk4,
        Self::Plasma,
        Self::Qt6,
        Self::Xfce,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Gnome => "GNOME / libadwaita",
            Self::Gtk3 => "GTK 3",
            Self::Gtk4 => "GTK 4",
            Self::Plasma => "KDE Plasma 6",
            Self::Qt6 => "Qt 6 (qt6ct)",
            Self::Xfce => "Xfce (Wayland)",
        }
    }
    fn relative(self) -> &'static str {
        match self {
            Self::Gnome => "dconf/user",
            Self::Gtk3 => "gtk-3.0/settings.ini",
            Self::Gtk4 => "gtk-4.0/settings.ini",
            Self::Plasma => "kdeglobals",
            Self::Qt6 => "qt6ct/qt6ct.conf",
            Self::Xfce => "xfce4/xfconf/xfce-perchannel-xml/xsettings.xml",
        }
    }
    pub fn supported(self) -> bool {
        if !wayland() {
            return false;
        }
        let desktop = env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_default()
            .to_ascii_lowercase();
        match self {
            Self::Gnome => executable("gsettings"),
            Self::Plasma => desktop.contains("kde") && executable("plasma-apply-colorscheme"),
            Self::Qt6 => env::var("QT_QPA_PLATFORMTHEME").as_deref() == Ok("qt6ct"),
            Self::Xfce => desktop.contains("xfce") && executable("xfconf-query"),
            _ => true,
        }
    }
}

pub fn wayland() -> bool {
    env::var("XDG_SESSION_TYPE").as_deref() == Ok("wayland")
        || env::var_os("WAYLAND_DISPLAY").is_some()
}

fn executable(name: &str) -> bool {
    env::var_os("PATH")
        .is_some_and(|paths| env::split_paths(&paths).any(|p| p.join(name).is_file()))
}

fn command(name: &str, args: &[&str]) -> io::Result<String> {
    let output = Command::new(name).args(args).output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn preference(value: &str) -> Option<Mode> {
    if value.contains("prefer-dark") {
        Some(Mode::Dark)
    } else if value.contains("prefer-light") {
        Some(Mode::Light)
    } else {
        None
    }
}

/// Portal Settings is read-only. This is status, never a writable backend.
pub fn portal_preference() -> Option<Mode> {
    let result = command(
        "gdbus",
        &[
            "call",
            "--session",
            "--timeout",
            "2",
            "--dest",
            "org.freedesktop.portal.Desktop",
            "--object-path",
            "/org/freedesktop/portal/desktop",
            "--method",
            "org.freedesktop.portal.Settings.ReadOne",
            "org.freedesktop.appearance",
            "color-scheme",
        ],
    )
    .ok()?;
    if result.contains("uint32 1") {
        Some(Mode::Dark)
    } else if result.contains("uint32 2") {
        Some(Mode::Light)
    } else {
        None
    }
}
#[derive(Clone)]
pub struct Paths {
    user: PathBuf,
    system: Vec<PathBuf>,
}

impl Paths {
    pub fn discover() -> io::Result<Self> {
        let home = env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))?;
        let user = env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home.join(".config"));
        let mut system: Vec<PathBuf> = env::var_os("XDG_CONFIG_DIRS")
            .map(|v| env::split_paths(&v).filter(|p| p.is_absolute()).collect())
            .unwrap_or_else(|| vec![PathBuf::from("/etc/xdg")]);
        system.push(PathBuf::from("/etc"));
        Ok(Self { user, system })
    }

    pub fn user(&self, target: Target) -> PathBuf {
        self.user.join(target.relative())
    }

    /// Only explicit system config is a default; absent settings are not fabricated.
    pub fn default(&self, target: Target) -> io::Result<Option<Mode>> {
        if target == Target::Gnome {
            // GSettings schema defaults are not administrator preferences.
            // Only an explicitly deployed dconf system database is a default here.
            let profile = env::var("DCONF_PROFILE").unwrap_or_else(|_| "user".into());
            let profile_file = PathBuf::from("/etc/dconf/profile").join(profile);
            if let Ok(contents) = fs::read_to_string(profile_file) {
                for db in contents
                    .lines()
                    .filter_map(|l| l.trim().strip_prefix("system-db:"))
                {
                    let root = PathBuf::from("/etc/dconf/db").join(format!("{db}.d"));
                    if let Ok(entries) = fs::read_dir(root) {
                        for entry in entries.flatten() {
                            if let Ok(s) = fs::read_to_string(entry.path()) {
                                if let Some(mode) =
                                    section_value(&s, "org/gnome/desktop/interface", "color-scheme")
                                        .and_then(preference)
                                {
                                    return Ok(Some(mode));
                                }
                            }
                        }
                    }
                }
            }
            return Ok(None);
        }
        for root in &self.system {
            let path = root.join(target.relative());
            if let Ok(contents) = fs::read_to_string(&path) {
                if target == Target::Xfce {
                    if let Some(mode) = xfce_system_theme(&contents).and_then(xfce_mode) {
                        return Ok(Some(mode));
                    }
                }
                if target == Target::Plasma {
                    return Ok(
                        section_value(&contents, "General", "ColorScheme").and_then(kde_mode)
                    );
                }
                if target == Target::Qt6 {
                    if let Some(palette_path) =
                        section_value(&contents, "Appearance", "color_scheme_path")
                    {
                        let palette_path = PathBuf::from(palette_path);
                        if palette_path.is_absolute()
                            && let Ok(colors) = fs::read_to_string(palette_path)
                            && let Some(background) =
                                section_value(&colors, "ColorScheme", "active_colors")
                                    .and_then(|v| v.split(',').nth(1))
                            && let Some(rgb) = background.trim().strip_prefix("#ff")
                            && rgb.len() == 6
                            && let Ok(luma) = u32::from_str_radix(rgb, 16)
                        {
                            return Ok(Some(if (luma >> 16) & 0xff < 128 {
                                Mode::Dark
                            } else {
                                Mode::Light
                            }));
                        }
                    }
                }
                if let Some(mode) = parse(target, &contents) {
                    return Ok(Some(mode));
                }
            }
        }
        Ok(None)
    }

    pub fn current(&self, target: Target) -> io::Result<Option<Mode>> {
        if target == Target::Gnome {
            return Ok(preference(&command(
                "gsettings",
                &["get", "org.gnome.desktop.interface", "color-scheme"],
            )?));
        }
        if target == Target::Plasma {
            return Ok(fs::read_to_string(self.user(target))
                .ok()
                .and_then(|s| section_value(&s, "General", "ColorScheme").and_then(kde_mode)));
        }
        if target == Target::Xfce {
            return Ok(
                command("xfconf-query", &["-c", "xsettings", "-p", "/Net/ThemeName"])
                    .ok()
                    .and_then(|s| xfce_mode(&s)),
            );
        }
        match fs::read_to_string(self.user(target)) {
            Ok(contents) if target == Target::Qt6 => {
                if section_value(&contents, "Appearance", "color_scheme_path")
                    != Some(self.palette().to_string_lossy().as_ref())
                {
                    return Ok(None);
                }
                Ok(fs::read_to_string(self.palette()).ok().and_then(|s| {
                    if s.starts_with("# sste:dark") {
                        Some(Mode::Dark)
                    } else if s.starts_with("# sste:light") {
                        Some(Mode::Light)
                    } else {
                        None
                    }
                }))
            }
            Ok(contents) => Ok(parse(target, &contents)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub fn exists(&self, target: Target) -> bool {
        if target == Target::Gnome {
            return self.current(target).ok().flatten().is_some();
        }
        if target == Target::Plasma {
            return fs::read_to_string(self.user(target)).is_ok_and(|s| {
                section_value(&s, "General", "ColorScheme").is_some()
                    || s.lines().any(|line| {
                        line.starts_with("[Colors:") || line.starts_with("[ColorEffects:")
                    })
            });
        }
        if target == Target::Xfce {
            return command("xfconf-query", &["-c", "xsettings", "-p", "/Net/ThemeName"]).is_ok();
        }
        fs::symlink_metadata(self.user(target)).is_ok()
            || target == Target::Qt6 && fs::symlink_metadata(self.palette()).is_ok()
    }

    pub fn writable(&self, target: Target) -> bool {
        if target == Target::Gnome {
            return !self.user.join("dconf/user").is_symlink()
                && command(
                    "gsettings",
                    &["writable", "org.gnome.desktop.interface", "color-scheme"],
                )
                .ok()
                .as_deref()
                    == Some("true");
        }
        if target == Target::Xfce {
            return !self.user(target).is_symlink();
        }
        let path = self.user(target);
        !self.user.is_symlink()
            && !path.is_symlink()
            && path.parent().is_some_and(|p| !p.is_symlink())
            && (target != Target::Qt6
                || (!self.palette().is_symlink()
                    && !self.palette().is_dir()
                    && !self.palette().parent().unwrap().is_symlink()))
            && !path.is_dir()
    }

    pub fn apply(&self, target: Target, mode: Mode) -> io::Result<()> {
        if target == Target::Gnome {
            if !self.writable(target) {
                return Err(io::Error::other(
                    "GNOME preference is locked or Nix-managed",
                ));
            }
            command(
                "gsettings",
                &[
                    "set",
                    "org.gnome.desktop.interface",
                    "color-scheme",
                    if mode == Mode::Dark {
                        "prefer-dark"
                    } else {
                        "prefer-light"
                    },
                ],
            )?;
            return Ok(());
        }
        if target == Target::Xfce {
            if !self.writable(target) {
                return Err(io::Error::other("Xfce settings are Nix-managed"));
            }
            let name = self
                .xfce_scheme(mode)
                .ok_or_else(|| io::Error::other("No installed matching Xfce GTK theme"))?;
            command(
                "xfconf-query",
                &[
                    "-c",
                    "xsettings",
                    "-p",
                    "/Net/ThemeName",
                    "-n",
                    "-t",
                    "string",
                    "-s",
                    &name,
                ],
            )?;
            return Ok(());
        }
        if target == Target::Plasma {
            if !self.writable(target) {
                return Err(io::Error::other("KDE settings are Nix-managed"));
            }
            let name = self
                .kde_scheme(mode)
                .ok_or_else(|| io::Error::other("No installed KDE color scheme for this mode"))?;
            command("plasma-apply-colorscheme", &[&name])?;
            return Ok(());
        }
        if !self.writable(target) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Nix-managed link or directory; edit your declarative configuration instead",
            ));
        }
        let path = self.user(target);
        let previous = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e),
        };
        let new = if target == Target::Qt6 {
            let updated = replace_key(
                &previous,
                "Appearance",
                "color_scheme_path",
                &self.palette().to_string_lossy(),
            );
            replace_key(&updated, "Appearance", "custom_palette", "true")
        } else {
            replace(target, &previous, mode)
        };
        if target == Target::Qt6 {
            fs::create_dir_all(self.palette().parent().unwrap())?;
            if !self.writable(target) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "Palette path changed while saving",
                ));
            }
            fs::write(self.palette(), palette(mode))?;
        }
        if new == previous {
            return Ok(());
        }
        let parent = path.parent().unwrap();
        fs::create_dir_all(parent)?;
        // create_new prevents overwriting a preexisting temporary file; rename is atomic.
        let tmp = parent.join(format!(
            ".sste-{}-{}.tmp",
            std::process::id(),
            target.relative().replace('/', "-")
        ));
        let result = (|| {
            use std::io::Write;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp)?;
            file.write_all(new.as_bytes())?;
            file.sync_all()?;
            // Refuse a link installed after we checked above.
            if !self.writable(target) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "Target changed while saving",
                ));
            }
            fs::rename(&tmp, &path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result
    }

    /// Cleaner: delete only the chosen target file and the app's Qt palette,
    /// not neighboring settings, directories, or Nix-managed symlinks.
    pub fn remove(&self, target: Target) -> io::Result<()> {
        if !self.writable(target) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Settings are Nix-managed or locked",
            ));
        }
        if target == Target::Gnome {
            command(
                "gsettings",
                &["reset", "org.gnome.desktop.interface", "color-scheme"],
            )?;
            return Ok(());
        }
        if target == Target::Xfce {
            command(
                "xfconf-query",
                &["-c", "xsettings", "-p", "/Net/ThemeName", "-r"],
            )?;
            return Ok(());
        }
        if target == Target::Plasma {
            let path = self.user(target);
            let previous = fs::read_to_string(&path)?;
            let cleaned = remove_kde_colors(&previous);
            if cleaned.trim().is_empty() || cleaned.trim() == "[General]" {
                fs::remove_file(path)?;
            } else {
                fs::write(path, cleaned)?;
            }
            // Let KDE invalidate its config cache for subsequently started applications.
            let _ = command("kbuildsycoca6", &["--noincremental"]);
            return Ok(());
        }
        if !self.writable(target) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Nix-managed link or directory; edit your declarative configuration instead",
            ));
        }
        if target == Target::Qt6 {
            let palette = self.palette();
            if palette.is_symlink() || palette.is_dir() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "Palette path is managed externally",
                ));
            }
            match fs::remove_file(palette) {
                Ok(()) => (),
                Err(e) if e.kind() == io::ErrorKind::NotFound => (),
                Err(e) => return Err(e),
            }
        }
        match fs::remove_file(self.user(target)) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            result => result,
        }
    }

    fn palette(&self) -> PathBuf {
        self.user
            .join("qt6ct/colors/super-simple-theme-enforcer.conf")
    }

    pub fn can_apply_mode(&self, target: Target, mode: Mode) -> bool {
        self.writable(target)
            && match target {
                Target::Plasma => self.kde_scheme(mode).is_some(),
                Target::Xfce => self.xfce_scheme(mode).is_some(),
                _ => true,
            }
    }

    fn kde_scheme(&self, mode: Mode) -> Option<String> {
        let name = match mode {
            Mode::Dark => "BreezeDark",
            Mode::Light => "BreezeLight",
        };
        command("plasma-apply-colorscheme", &["--list-schemes"])
            .ok()
            .filter(|s| s.contains(name))
            .map(|_| name.to_owned())
    }

    fn xfce_scheme(&self, mode: Mode) -> Option<String> {
        let current = command("xfconf-query", &["-c", "xsettings", "-p", "/Net/ThemeName"])
            .ok()
            .or_else(|| {
                self.system.iter().find_map(|root| {
                    fs::read_to_string(root.join(Target::Xfce.relative()))
                        .ok()
                        .and_then(|xml| xfce_system_theme(&xml).map(str::to_owned))
                })
            })?;
        let base = current
            .strip_suffix("-dark")
            .or_else(|| current.strip_suffix("-Dark"))
            .unwrap_or(&current);
        let wanted = if mode == Mode::Dark {
            format!("{base}-dark")
        } else {
            base.to_owned()
        };
        let roots = env::var_os("XDG_DATA_DIRS")
            .map(|v| env::split_paths(&v).collect::<Vec<_>>())
            .unwrap_or_else(|| {
                vec![
                    PathBuf::from("/run/current-system/sw/share"),
                    PathBuf::from("/usr/share"),
                ]
            });
        if roots.into_iter().any(|p| {
            p.join("themes")
                .join(&wanted)
                .join("gtk-3.0/gtk.css")
                .exists()
        }) || env::var_os("HOME").is_some_and(|p| {
            PathBuf::from(p)
                .join(".themes")
                .join(&wanted)
                .join("gtk-3.0/gtk.css")
                .exists()
        }) {
            Some(wanted)
        } else {
            None
        }
    }
}

fn kde_mode(name: &str) -> Option<Mode> {
    match name.trim() {
        "BreezeDark" => Some(Mode::Dark),
        "BreezeLight" | "Breeze" => Some(Mode::Light),
        _ => None,
    }
}

fn xfce_mode(name: &str) -> Option<Mode> {
    let lower = name.to_ascii_lowercase();
    if lower.contains("dark") {
        Some(Mode::Dark)
    } else if lower.contains("light") || lower == "adwaita" || lower == "greybird" || lower == "arc"
    {
        Some(Mode::Light)
    } else {
        None
    }
}

/// Read the one xfconf property we need, without touching the user XML database.
fn xfce_system_theme(xml: &str) -> Option<&str> {
    let net = xml.find("<property name=\"Net\"")?;
    let body = xml.get(net..)?;
    let end = body.find("</property>")?;
    let theme = body[..end].find("<property name=\"ThemeName\"")?;
    let property = &body[theme..];
    let closing = property.find("/>")?;
    let value = property[..closing].split("value=\"").nth(1)?;
    value.split('"').next()
}

fn remove_kde_colors(old: &str) -> String {
    let mut out = String::new();
    let mut drop = false;
    let mut general = false;
    for line in old.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            drop = trimmed.starts_with("[Colors:") || trimmed.starts_with("[ColorEffects:");
            general = trimmed == "[General]";
        }
        if !drop && !(general && trimmed.starts_with("ColorScheme=")) {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

fn section_value<'a>(src: &'a str, section: &str, key: &str) -> Option<&'a str> {
    let mut inside = false;
    for line in src.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            inside = line[1..line.len() - 1].eq_ignore_ascii_case(section);
        } else if inside {
            if let Some((k, v)) = line.split_once('=')
                && k.trim().eq_ignore_ascii_case(key)
            {
                return Some(v.trim());
            }
        }
    }
    None
}

fn parse(target: Target, src: &str) -> Option<Mode> {
    match target {
        Target::Gtk3 | Target::Gtk4 => {
            match section_value(src, "Settings", "gtk-application-prefer-dark-theme")? {
                "1" | "true" => Some(Mode::Dark),
                "0" | "false" => Some(Mode::Light),
                _ => None,
            }
        }
        Target::Qt6 | Target::Gnome | Target::Plasma | Target::Xfce => None, // These backends have their own preference readers.
    }
}

fn replace(target: Target, old: &str, mode: Mode) -> String {
    let _ = target;
    replace_key(
        old,
        "Settings",
        "gtk-application-prefer-dark-theme",
        if mode == Mode::Dark { "1" } else { "0" },
    )
}

fn replace_key(old: &str, section: &str, key: &str, value: &str) -> String {
    let mut output = String::new();
    let mut inside = false;
    let mut found_section = false;
    let mut found_key = false;
    for line in old.lines() {
        if line.trim().starts_with('[') && line.trim().ends_with(']') {
            if inside && !found_key {
                output.push_str(&format!("{key}={value}\n"));
                found_key = true;
            }
            inside = line.trim()[1..line.trim().len() - 1].eq_ignore_ascii_case(section);
            found_section |= inside;
        }
        if inside
            && line
                .split_once('=')
                .is_some_and(|(k, _)| k.trim().eq_ignore_ascii_case(key))
        {
            output.push_str(&format!("{key}={value}\n"));
            found_key = true;
        } else {
            output.push_str(line);
            output.push('\n');
        }
    }
    if !found_section {
        output.push_str(&format!("[{section}]\n"));
    }
    if !found_key {
        output.push_str(&format!("{key}={value}\n"));
    }
    output
}

fn palette(mode: Mode) -> String {
    let (fg, bg, mid, accent) = match mode {
        Mode::Dark => ("#ffe8f2f2", "#ff202a2a", "#ff394949", "#ff008080"),
        Mode::Light => ("#ff203333", "#fff6fafa", "#ffd3e0e0", "#ff008080"),
    };
    // Qt palette order follows qt6ct's [ColorScheme] active/disabled/inactive colors.
    let roles = [
        fg, bg, bg, fg, mid, mid, fg, fg, fg, bg, mid, mid, mid, fg, accent, accent, mid, fg, bg,
        fg, fg,
    ];
    let colors = roles.join(", ");
    format!(
        "# sste:{}\n[ColorScheme]\nactive_colors={colors}\ndisabled_colors={colors}\ninactive_colors={colors}\n",
        mode.label().to_lowercase()
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gtk_preserves_other_keys() {
        let result = replace(
            Target::Gtk3,
            "[Settings]\ngtk-theme-name=Mine\ngtk-application-prefer-dark-theme=0\n[Other]\na=b\n",
            Mode::Dark,
        );
        assert!(result.contains("gtk-theme-name=Mine"));
        assert!(result.contains("gtk-application-prefer-dark-theme=1"));
        assert!(result.contains("[Other]\na=b"));
    }
    #[test]
    fn remove_is_scoped() {
        let base = env::temp_dir().join(format!("sste-test-{}", std::process::id()));
        let paths = Paths {
            user: base.join("user"),
            system: vec![base.join("system")],
        };
        fs::create_dir_all(paths.user.join("gtk-3.0")).unwrap();
        fs::write(paths.user.join("gtk-3.0/other.css"), "keep").unwrap();
        paths.apply(Target::Gtk3, Mode::Dark).unwrap();
        paths.remove(Target::Gtk3).unwrap();
        assert!(paths.user.join("gtk-3.0/other.css").exists());
        assert!(!paths.exists(Target::Gtk3));
        fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn qt_palette_and_cleaner() {
        let base = env::temp_dir().join(format!("sste-qt-test-{}", std::process::id()));
        let paths = Paths {
            user: base.join("user"),
            system: vec![base.join("system")],
        };
        fs::create_dir_all(paths.user.join("qt6ct")).unwrap();
        fs::write(
            paths.user.join("qt6ct/qt6ct.conf"),
            "[Fonts]\nfamily=Example\n",
        )
        .unwrap();
        paths.apply(Target::Qt6, Mode::Dark).unwrap();
        assert_eq!(paths.current(Target::Qt6).unwrap(), Some(Mode::Dark));
        paths.apply(Target::Qt6, Mode::Light).unwrap();
        assert_eq!(paths.current(Target::Qt6).unwrap(), Some(Mode::Light));
        assert!(
            fs::read_to_string(paths.user(Target::Qt6))
                .unwrap()
                .contains("family=Example")
        );
        paths.remove(Target::Qt6).unwrap();
        assert!(!paths.palette().exists());
        assert!(!paths.user(Target::Qt6).exists());
        fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn no_invented_system_default() {
        let paths = Paths {
            user: PathBuf::from("/nonexistent/sste-user"),
            system: vec![PathBuf::from("/nonexistent/sste-system")],
        };
        assert_eq!(paths.default(Target::Gtk3).unwrap(), None);
    }
    #[test]
    fn kde_cleanup_preserves_unrelated_settings() {
        let old = "[General]\nfont=Noto Sans\nColorScheme=BreezeDark\naccentColor=#112233\n\
            [Colors:Window]\nBackgroundNormal=10,10,10\n\
            [ColorEffects:Disabled]\nIntensityAmount=0.1\n\
            [KDE]\nSingleClick=false\n";
        let cleaned = remove_kde_colors(old);
        assert!(cleaned.contains("font=Noto Sans"));
        assert!(cleaned.contains("accentColor=#112233"));
        assert!(cleaned.contains("[KDE]\nSingleClick=false"));
        assert!(!cleaned.contains("ColorScheme="));
        assert!(!cleaned.contains("Colors:Window"));
        assert!(!cleaned.contains("IntensityAmount"));
    }
    #[test]
    fn xfce_system_default_is_explicit() {
        let xml = r#"<channel name="xsettings">
          <property name="Net" type="empty">
            <property name="ThemeName" type="string" value="Adwaita-dark"/>
          </property></channel>"#;
        let base = env::temp_dir().join(format!("sste-xfce-test-{}", std::process::id()));
        let paths = Paths {
            user: base.join("user"),
            system: vec![base.join("system")],
        };
        let system = paths.system[0].join(Target::Xfce.relative());
        fs::create_dir_all(system.parent().unwrap()).unwrap();
        fs::write(system, xml).unwrap();
        assert_eq!(paths.default(Target::Xfce).unwrap(), Some(Mode::Dark));
        fs::remove_dir_all(base).unwrap();
    }
}
