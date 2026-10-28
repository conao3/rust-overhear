{
  pkgs,
  crane,
  aprilLibDir,
  desktopItem,
}:
let
  inherit (pkgs) lib;
  craneLib = (crane.mkLib pkgs).overrideToolchain pkgs.rustToolchain;
  source =
    fileset:
    lib.fileset.toSource {
      root = ../.;
      inherit fileset;
    };
  rustSrc = source (
    lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../crates/overhear-core/data
      (lib.fileset.fileFilter (file: file.hasExt "rs" || file.name == "Cargo.toml") ../crates)
      (lib.fileset.fileFilter (file: file.hasExt "rs" || file.name == "Cargo.toml") ../src-tauri)
      ../src-tauri/tauri.conf.json
      ../src-tauri/capabilities
      ../src-tauri/icons
    ]
  );
  frontendSrc = source (
    lib.fileset.unions [
      ../package.json
      ../pnpm-lock.yaml
      ../tsconfig.json
      ../tsconfig.node.json
      ../vite.config.ts
      ../index.html
      ../caption.html
      ../src
    ]
  );
  frontend = pkgs.stdenvNoCC.mkDerivation (finalAttrs: {
    pname = "overhear-frontend";
    version = "0.1.0";
    src = frontendSrc;
    pnpmDeps = pkgs.fetchPnpmDeps {
      inherit (finalAttrs) pname version;
      src = source (
        lib.fileset.unions [
          ../package.json
          ../pnpm-lock.yaml
        ]
      );
      pnpm = pkgs.overhearPnpm;
      fetcherVersion = 4;
      hash = "sha256-WiN3Vwv1RgUshh9eFpi4VjwaIVCETLbdKpyn8c9mrTE=";
    };
    nativeBuildInputs = with pkgs; [
      overhearNodejs
      overhearPnpm
      pnpmConfigHook
    ];
    buildPhase = ''
      runHook preBuild
      pnpm build
      runHook postBuild
    '';
    installPhase = ''
      runHook preInstall
      cp -r dist $out
      runHook postInstall
    '';
  });
  commonArgs = {
    pname = "overhear";
    version = "0.1.0";
    src = rustSrc;
    cargoVendorDir = pkgs.rustPlatform.importCargoLock { lockFile = ../Cargo.lock; };
    cargoExtraArgs = "--locked --workspace --features overhear/custom-protocol";
    nativeBuildInputs = with pkgs; [
      pkg-config
      wrapGAppsHook3
      makeWrapper
    ];
    buildInputs = with pkgs; [
      openssl
      sqlite
      dbus
      webkitgtk_4_1
      gtk3
      libsoup_3
      glib-networking
      librsvg
      libayatana-appindicator
    ];
    APRIL_LIB_DIR = aprilLibDir;
    strictDeps = true;
  };
  cargoArtifacts = craneLib.buildDepsOnly (
    commonArgs
    // {
      buildPhaseCargoCommand = "cargoWithProfile build ${commonArgs.cargoExtraArgs}";
    }
  );
  unwrappedPackage = craneLib.buildPackage (
    commonArgs
    // {
      inherit cargoArtifacts;
      preBuild = ''
        cp -r ${frontend} dist
      '';
      preCheck = ''
        export EJDICT_PATH="${pkgs.ejdict}/ejdict-hand-utf8.txt"
        export WORDNET_DICT_DIR="${pkgs.wordnet}/dict"
        export WORDNET_JA_PATH="${pkgs.wordnet-ja}"
        export IPA_US_PATH="${pkgs.ipa-us}"
      '';
      postInstall = ''
        for bin in overhear overhear-server; do
          wrapProgram $out/bin/$bin \
            --argv0 "$bin" \
            --set APRIL_MODEL_PATH "${pkgs.april-model}" \
            --set WHISPER_MODEL_PATH "${pkgs.whisper-model}" \
            --set WORDNET_DICT_DIR "${pkgs.wordnet}/dict" \
            --set WORDNET_JA_PATH "${pkgs.wordnet-ja}" \
            --set EJDICT_PATH "${pkgs.ejdict}/ejdict-hand-utf8.txt" \
            --set IPA_US_PATH "${pkgs.ipa-us}" \
            --set OVERHEAR_AUTOSTART_EXEC overhear \
            --set-default WEBKIT_DISABLE_DMABUF_RENDERER 1 \
            --prefix LD_LIBRARY_PATH : "${pkgs.libayatana-appindicator}/lib" \
            --prefix PATH : "${
              lib.makeBinPath [
                pkgs.pipewire
                pkgs.whisper-cpp
              ]
            }"
        done
        install -Dm644 ${../THIRD_PARTY_LICENSES.txt} $out/share/licenses/overhear/THIRD_PARTY_LICENSES.txt
        install -Dm644 src-tauri/icons/128x128.png $out/share/icons/hicolor/128x128/apps/overhear.png
        install -Dm644 ${desktopItem}/share/applications/overhear.desktop $out/share/applications/overhear.desktop
      '';
      passthru = { inherit cargoArtifacts frontend; };
      meta = {
        description = "システム音声を常時文字起こしする語学学習用デスクトップアプリ";
        homepage = "https://github.com/conao3/rust-overhear";
        license = lib.licenses.gpl3Only;
        mainProgram = "overhear";
        platforms = [ "x86_64-linux" ];
      };
    }
  );
  playbackPluginPath = lib.makeSearchPath "lib/gstreamer-1.0" (
    with pkgs.gst_all_1;
    [
      gstreamer.out
      gst-plugins-base
      gst-plugins-good
      gst-plugins-bad
    ]
  );
  package = pkgs.symlinkJoin {
    inherit (unwrappedPackage) name meta;
    paths = [ unwrappedPackage ];
    nativeBuildInputs = [ pkgs.makeWrapper ];
    postBuild = ''
      wrapProgram $out/bin/overhear \
        --prefix GST_PLUGIN_SYSTEM_PATH_1_0 : "${playbackPluginPath}"
      export GST_PLUGIN_SYSTEM_PATH_1_0="${playbackPluginPath}"
      export GST_PLUGIN_PATH_1_0=
      export GST_REGISTRY_1_0="$TMPDIR/gstreamer-registry.bin"
      for element in typefind queue fakesink appsink autoaudiosink pulsesink wavparse audioconvert audioresample decodebin playbin fakevideosink webvttenc; do
        ${lib.getBin pkgs.gst_all_1.gstreamer}/bin/gst-inspect-1.0 "$element" > /dev/null
        echo "GStreamer playback element: $element"
      done
    '';
    passthru = { inherit (unwrappedPackage) cargoArtifacts frontend; };
  };

in
{
  default = package;
  inherit frontend;
  rust-deps = cargoArtifacts;
}
