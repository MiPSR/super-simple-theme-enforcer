# Super Simple Theme Enforcer

A small NixOS/Wayland-only, user-side theme switcher and cleaner. Rust + Rune, with
the same teal accent and layout as [Super Simple Defaults](https://github.com/MiPSR/super-simple-defaults).

## Install

```nix
inputs.super-simple-theme-enforcer.url = "github:MiPSR/super-simple-theme-enforcer";
environment.systemPackages = [
  inputs.super-simple-theme-enforcer.packages.x86_64-linux.default
];
```

Or run `nix run github:MiPSR/super-simple-theme-enforcer`.

Check the components, choose sun (light) or moon (dark), and press **Apply**.
The choice is pending until Apply; it does not preview or recolor this window.
The window follows Rune's system appearance (`Cx::system_dark()`), which on
Linux reads the appearance portal and follows its `SettingChanged` signal.

Each theme dropdown appears only while its corresponding component is checked
and is enabled when installed themes of that toolkit are available. GTK 3 and GTK 4 are discovered
separately. KDE color schemes and qt6ct palettes are separate formats, not
interchangeable. The Qt widget-style dropdown is absent unless Qt or KDE is
checked; its entries come from Qt's actual `QStyleFactory` keys via the bundled
read-only `sste-qt-styles` helper. Choices are kept separately for light and dark.
An explicit theme/palette choice takes precedence over the automatic palette;
choose one appropriate for the selected mode.

Discovery includes XDG data/config paths, `~/.themes`, the user Nix profile,
the NixOS system profile and the per-user NixOS profile. The app does not install
themes, set session environment variables, replace platform plugins, or modify
NixOS/Home Manager declarations. Unavailable components start unchecked, with
the missing command, unavailable service, or locked configuration on their row's
disabled status button. Available checkboxes can always be unchecked.

**Defaults** applies
only explicit administrator preferences (XDG configuration or deployed dconf);
it is disabled if no selected component has a usable system preference.
**Delete overrides** shows a confirmation with the exact selected user paths and deletes
the GTK/qt6ct files even if they predate this app. For shared settings stores
it resets GNOME's single color-scheme key and Xfce's single theme property;
for KDE it removes theme-specific entries in `kdeglobals`, not the entire
file. It never deletes whole directories, other applications' files, or
Nix-managed symlinks. **OK** and **Close** both
close the window; Apply, Defaults and confirmed Delete save immediately.

Supported targets (paths under `XDG_CONFIG_HOME`, normally `~/.config`):

- GNOME/libadwaita: `org.gnome.desktop.interface/color-scheme` via GSettings.
- GTK 3: `gtk-3.0/settings.ini`, including the selected `gtk-theme-name`.
- GTK 4: `gtk-4.0/settings.ini`, including the selected `gtk-theme-name`.
- KDE / Kirigami: installed `.colors` schemes are merged into `kdeglobals`,
  including `[General] ColorScheme`, `[UiSettings] ColorScheme`, `[Colors:*]`,
  and `[ColorEffects:*]`. The selected Qt widget style writes `[KDE] widgetStyle`.
  This works without a Plasma session or `plasma-apply-colorscheme`.
  Apply/Delete sends KDE's palette-change notification when `dbus-send` is
  available; applications that do not reload global settings need restarting.
- Qt 6 (qt6ct): `qt6ct/qt6ct.conf` and the app-owned palette at
  `qt6ct/colors/super-simple-theme-enforcer.conf`.
- Xfce Wayland: `/Net/ThemeName` via `xfconf-query`, using a selected GTK theme
  or an installed light/dark variant of the current theme.

Qt 6 configuration is selectable without an environment-variable gate. Writing
qt6ct settings does not make applications load qt6ct: apps using another platform
integration may ignore those files. This app does not install or enable qt6ct.
GTK settings are hints; GNOME/libadwaita uses the separate GNOME preference.
The XDG portal's read-only preference is shown for reference, never written.
Qt applications using KDE's platform theme follow Plasma instead of qt6ct.
There is no X11/Xorg, GTK 2 or Qt 5 integration.
Nix/Home Manager symlinks and declarative settings must be edited in your
configuration. Existing unrelated keys are preserved by Apply; Delete overrides
intentionally deletes the *whole selected GTK/qt6ct files*, after confirmation.
External selected palette files are only referenced, never rewritten or deleted.
An application's explicit private theme setting, sandbox-private configuration,
or hardcoded style can override global configuration; this app does not edit
unrelated application files or sandbox permissions.

### Why KDE is separate from qt6ct

Ricing reports such as [this Hyprland guide](https://www.lorenzobettini.it/2024/08/better-kde-theming-and-styling-in-hyprland/)
and [the Kate discussion](https://discuss.kde.org/t/how-to-get-kate-to-respect-qt6ct-window-color-theme-again/28948)
point to a distinction still present in current upstream code:
[KColorSchemeManager](https://github.com/KDE/kcolorscheme/blob/master/src/kcolorschememanager.cpp)
reads `UiSettings/ColorScheme`, while KDE/Kirigami also need full KDE color groups.
[Kate/KWrite](https://github.com/KDE/kate/blob/master/apps/lib/kateapp.cpp)
use KDE's style infrastructure; [WiVRn](https://github.com/WiVRn/WiVRn/blob/master/dashboard/main.cpp)
explicitly selects Breeze and normally `org.kde.desktop` Qt Quick controls.
A qt6ct QPalette alone therefore is not a universal KDE theme override.

## Development

`nix develop` then `cargo test`.
