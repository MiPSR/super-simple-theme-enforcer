{
  description = "Super Simple Theme Enforcer — user-side theme controls for NixOS";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
      libs = with pkgs; [
        wayland libxkbcommon vulkan-loader fontconfig freetype expat
        libGL
      ];
      libPath = pkgs.lib.makeLibraryPath libs;
    in {
      packages.${system}.default = pkgs.rustPlatform.buildRustPackage {
        pname = "super-simple-theme-enforcer";
        version = "0.1.0";
        src = pkgs.lib.cleanSource ./.;
        cargoHash = "sha256-CXe68zvDJSKJUVmpCAK9XTxjSgQDnHUm09BaJeZRb+c=";
        nativeBuildInputs = [ pkgs.pkg-config pkgs.makeWrapper pkgs.desktop-file-utils ];
        buildInputs = libs;
        postInstall = ''
          install -Dm644 assets/io.github.MiPSR.SuperSimpleThemeEnforcer.desktop \
            $out/share/applications/io.github.MiPSR.SuperSimpleThemeEnforcer.desktop
          install -Dm644 assets/super-simple-theme-enforcer.svg \
            $out/share/icons/hicolor/scalable/apps/io.github.MiPSR.SuperSimpleThemeEnforcer.svg
          desktop-file-validate $out/share/applications/io.github.MiPSR.SuperSimpleThemeEnforcer.desktop
          ln -s sste $out/bin/super-simple-theme-enforcer
        '';
        postFixup = ''
          wrapProgram $out/bin/sste \
            --prefix LD_LIBRARY_PATH : "${libPath}" \
            --prefix PATH : "${pkgs.lib.makeBinPath [ pkgs.glib ]}" \
            --prefix GIO_EXTRA_MODULES : "${pkgs.dconf.lib}/lib/gio/modules" \
            --set GSETTINGS_SCHEMA_DIR "${pkgs.gsettings-desktop-schemas}/share/gsettings-schemas/${pkgs.gsettings-desktop-schemas.name}/glib-2.0/schemas"
        '';
        meta = {
          description = "User-side light and dark theme controller and cleaner";
          mainProgram = "sste";
          platforms = [ system ];
        };
      };
      apps.${system}.default = {
        type = "app";
        program = "${self.packages.${system}.default}/bin/sste";
      };
      devShells.${system}.default = pkgs.mkShell {
        buildInputs = [ pkgs.rustc pkgs.cargo pkgs.rustfmt pkgs.clippy pkgs.gcc pkgs.pkg-config pkgs.glib pkgs.gsettings-desktop-schemas ] ++ libs;
        shellHook = ''
          export LD_LIBRARY_PATH="${libPath}:$LD_LIBRARY_PATH"
        '';
      };
      checks.${system}.default = self.packages.${system}.default;
    };
}
