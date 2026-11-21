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

Select the components and Light or Dark, then **Apply**. **Defaults** applies
only explicit administrator preferences (XDG configuration or deployed dconf);
it is disabled if no selected component has a usable system preference.
**Remove** shows a confirmation with the exact selected user paths and deletes
the GTK/qt6ct files even if they predate this app. For shared settings stores
it resets GNOME's single color-scheme key and Xfce's single theme property;
for Plasma it removes theme-specific entries in `kdeglobals`, not the entire
file. It never deletes whole directories, other applications' files, or
Nix-managed symlinks. **OK** and **Close** both
close the window; operations take effect immediately.

Supported targets (paths under `XDG_CONFIG_HOME`, normally `~/.config`):

- GNOME/libadwaita: `org.gnome.desktop.interface/color-scheme` via GSettings.
- GTK 3: `gtk-3.0/settings.ini`
- GTK 4: `gtk-4.0/settings.ini`
- Plasma 6: installed Breeze light/dark schemes via `plasma-apply-colorscheme`;
  cleanup of `[General] ColorScheme`, `[Colors:*]` and `[ColorEffects:*]`
  overrides in `kdeglobals`. Available only in a KDE session. Applications
  already open may need restarting after cleanup.
- Qt 6 (qt6ct): `qt6ct/qt6ct.conf` and the app-owned palette at
  `qt6ct/colors/super-simple-theme-enforcer.conf`.
- Xfce Wayland: `/Net/ThemeName` via `xfconf-query` if a matching light/dark
  GTK theme is installed. Xfce's Wayland support is experimental and depends
  on a Wayland compositor.

Qt 6 is selectable only if `QT_QPA_PLATFORMTHEME=qt6ct` is already configured
in the desktop session. This app does not install or enable qt6ct for you.
GTK settings are hints; GNOME/libadwaita uses the separate GNOME preference.
The XDG portal's read-only preference is shown for reference, never written.
Qt applications using KDE's platform theme follow Plasma instead of qt6ct.
There is no X11/Xorg, GTK 2 or Qt 5 integration.
Nix/Home Manager symlinks and declarative settings must be edited in your
configuration. Existing unrelated keys are preserved by Apply; Remove
intentionally deletes the *whole selected GTK/qt6ct files*, after confirmation.

## Development

`nix develop` then `cargo test`.
