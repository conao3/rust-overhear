{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    treefmt-nix.url = "github:numtide/treefmt-nix";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs =
    inputs:
    inputs.flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [ "x86_64-linux" ];

      imports = [ inputs.treefmt-nix.flakeModule ];

      perSystem =
        { system, ... }:
        let
          overlay =
            final: prev:
            let
              nodejs = prev.nodejs_24;
              pnpm = prev.pnpm_10.override { nodejs-slim = prev.nodejs-slim_24; };
              rustToolchain = prev.rust-bin.stable.latest.default;

              # april-asr の学習済みモデル。livecaptions の overlay
              # (nixos-configuration) と同じ URL / hash を使うため、既に store に
              # あるものが再利用され再ダウンロードは発生しない。
              april-model = final.fetchurl {
                url = "https://april.sapples.net/april-english-dev-01110_en.april";
                hash = "sha256-d+uV0PpPdwijfoaMImUwHubELcsl5jymPuo9nLrbwfM=";
              };

              # two-pass ASR の後段。april の即時出力を、句読点つきの
              # 確定文へ差し替えるために使う。
              # 英和辞書 (パブリックドメイン)。
              ejdict = final.fetchzip {
                name = "ejdic-hand";
                url = "https://github.com/kujirahand/EJDict/releases/download/v2.0.1/ejdic-hand-txt.zip";
                hash = "sha256-vw9Qs3p01MTsi+VZ320/Sd+/IMibqtc7JEHiC9GjYzI=";
              };

              whisper-model = final.fetchurl {
                name = "ggml-base.en.bin";
                url = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin";
                hash = "sha256-oDd5yG3zMjB19eeWyyzlAp8A7Ihp7uP9+4l6/jbG0AI=";
              };
            in
            {
              inherit
                nodejs
                pnpm
                rustToolchain
                april-model
                whisper-model
                ejdict
                ;
            };

          pkgs = import inputs.nixpkgs {
            inherit system;
            overlays = [
              inputs.rust-overlay.overlays.default
              overlay
            ];
          };

          # libaprilasr.so は nixpkgs の livecaptions の出力に同梱されている。
          # april-asr 単体のパッケージは nixpkgs に無いため、これを直接リンクする。
          aprilLibDir = "${pkgs.livecaptions}/lib";

          desktopItem = pkgs.makeDesktopItem {
            name = "overhear";
            desktopName = "overhear";
            comment = "システム音声を字幕にして語学学習に使う";
            exec = "overhear";
            icon = "overhear";
            categories = [
              "Education"
              "AudioVideo"
            ];
          };
        in
        {
          packages.default = pkgs.rustPlatform.buildRustPackage (finalAttrs: {
            pname = "overhear";
            version = "0.1.0";
            src = ./.;

            cargoLock.lockFile = ./Cargo.lock;

            # これが無いと dist を埋め込まず、開発サーバ (localhost:1420) を
            # 見に行って真っ白になる。
            buildFeatures = [ "overhear/custom-protocol" ];

            pnpmDeps = pkgs.fetchPnpmDeps {
              inherit (finalAttrs) pname version src;
              inherit (pkgs) pnpm;
              fetcherVersion = 4;
              hash = "sha256-2CboHElg+2hEWtnrNVOZqEkW0P+/t2ZTLLMleFPENUA=";
            };

            nativeBuildInputs = with pkgs; [
              pkg-config
              nodejs
              pnpm
              pnpmConfigHook
              wrapGAppsHook3
              makeWrapper
            ];

            buildInputs = with pkgs; [
              openssl
              sqlite
              dbus # keyring (Secret Service)
              webkitgtk_4_1
              gtk3
              libsoup_3
              glib-networking
              librsvg
              libayatana-appindicator
            ];

            # tauri-build が dist/ を実行ファイルへ埋め込むので、
            # Rust のビルドより先にフロントを作る。
            preBuild = ''
              pnpm build
            '';

            # build.rs が libaprilasr の rpath を埋める。
            APRIL_LIB_DIR = aprilLibDir;

            # モデル・辞書と、子プロセスとして起動する外部コマンドを固定する。
            postInstall = ''
              # overhear-server は overhear の子として環境を継承するが、
              # 単体でも起動できるよう同じものを包んでおく。
              # WebKitGTK の DMA-BUF レンダラは Intel + X11 等で何も描画しない
              # (ウィンドウが真っ白になる) ことがあるので、既定で切る。
              for bin in overhear overhear-server; do
                # argv0 を保たないと WM class が .overhear-wrapped になり、
                # desktop entry の StartupWMClass と噛み合わない。
                wrapProgram $out/bin/$bin \
                  --argv0 "$bin" \
                  --set APRIL_MODEL_PATH "${pkgs.april-model}" \
                  --set WHISPER_MODEL_PATH "${pkgs.whisper-model}" \
                  --set WORDNET_DICT_DIR "${pkgs.wordnet}/dict" \
                  --set EJDICT_PATH "${pkgs.ejdict}/ejdict-hand-utf8.txt" \
                  --set OVERHEAR_AUTOSTART_EXEC overhear \
                  --set-default WEBKIT_DISABLE_DMABUF_RENDERER 1 \
                  --prefix LD_LIBRARY_PATH : "${pkgs.libayatana-appindicator}/lib" \
                  --prefix PATH : "${
                    pkgs.lib.makeBinPath [
                      pkgs.pipewire
                      pkgs.whisper-cpp
                    ]
                  }"
              done

              install -Dm644 src-tauri/icons/128x128.png \
                $out/share/icons/hicolor/128x128/apps/overhear.png
              install -Dm644 ${desktopItem}/share/applications/overhear.desktop \
                $out/share/applications/overhear.desktop
            '';

            meta = {
              description = "システム音声を常時文字起こしする語学学習用デスクトップアプリ";
              homepage = "https://github.com/conao3/rust-overhear";
              license = pkgs.lib.licenses.gpl3Only;
              mainProgram = "overhear";
              platforms = [ "x86_64-linux" ];
            };
          });

          devShells.default = pkgs.mkShell {
            packages = with pkgs; [
              rustToolchain
              nodejs
              pnpm
              pkg-config
              pipewire # pw-record (音声キャプチャ)
              sqlite
              wordnet
              whisper-cpp # whisper-server (two-pass ASR の後段)
            ];

            buildInputs = with pkgs; [
              openssl
              sqlite
              dbus # keyring (Secret Service)
              # Tauri (Linux)
              webkitgtk_4_1
              gtk3
              libsoup_3
              glib-networking
              librsvg
              libayatana-appindicator # トレイアイコン (dlopen される)
            ];

            env = {
              APRIL_LIB_DIR = aprilLibDir;
              # WordNet 3.0 の dict ファイル (英英辞書)。nixpkgs に入っているため
              # 追加のダウンロードは要らない。
              WORDNET_DICT_DIR = "${pkgs.wordnet}/dict";
              WHISPER_MODEL_PATH = "${pkgs.whisper-model}";
              EJDICT_PATH = "${pkgs.ejdict}/ejdict-hand-utf8.txt";
              APRIL_MODEL_PATH = "${pkgs.april-model}";
              # libaprilasr.so と libayatana-appindicator は dlopen されるので
              # 実行時の検索パスに載せる。
              LD_LIBRARY_PATH = "${aprilLibDir}:${pkgs.libayatana-appindicator}/lib";
              GIO_MODULE_PATH = "${pkgs.glib-networking}/lib/gio/modules";
            };
          };

          treefmt = {
            projectRootFile = "flake.nix";
            programs.nixfmt.enable = true;
            programs.rustfmt.enable = true;
            programs.prettier.enable = true;
            settings.global.excludes = [
              "pnpm-lock.yaml"
              "Cargo.lock"
            ];
          };
        };
    };
}
