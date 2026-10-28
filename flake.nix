{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    treefmt-nix.url = "github:numtide/treefmt-nix";
    rust-overlay.url = "github:oxalica/rust-overlay";
    crane.url = "github:ipetkov/crane";
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
              overhearNodejs = prev.nodejs_latest;
              overhearPnpm = prev.pnpm.override { nodejs-slim = prev.nodejs-slim_latest; };
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
                stripRoot = false;
                url = "https://github.com/kujirahand/EJDict/releases/download/v2.0.1/ejdic-hand-txt.zip";
                hash = "sha256-vw9Qs3p01MTsi+VZ320/Sd+/IMibqtc7JEHiC9GjYzI=";
              };

              ipa-us = final.fetchurl {
                name = "ipa-dict-en-US.txt";
                url = "https://raw.githubusercontent.com/open-dict-data/ipa-dict/43c3570eb3553bdd19fccd2bd0091534889af023/data/en_US.txt";
                hash = "sha256-KvbxVKXDYydfBS0fhazt7zjtGFypdFqkMUvnf2tw3mc=";
              };

              wordnet-ja = final.fetchurl {
                name = "wn-data-jpn.tab";
                url = "https://raw.githubusercontent.com/omwn/omw-data/406bf83b3c507a3d1f26e88252d5d66893fd36bf/wns/jpn/wn-data-jpn.tab";
                hash = "sha256-9H/vsRgkuG2FVsYy9bcUODl6useWd7YHBwzQvcuawhI=";
              };

              whisper-model = final.fetchurl {
                name = "ggml-base.en.bin";
                url = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin";
                hash = "sha256-oDd5yG3zMjB19eeWyyzlAp8A7Ihp7uP9+4l6/jbG0AI=";
              };
            in
            {
              inherit
                overhearNodejs
                overhearPnpm
                rustToolchain
                april-model
                whisper-model
                ejdict
                ipa-us
                wordnet-ja
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
          packages = import ./nix/package.nix {
            inherit pkgs aprilLibDir desktopItem;
            inherit (inputs) crane;
          };

          devShells.default = pkgs.mkShell {
            packages = with pkgs; [
              rustToolchain
              overhearNodejs
              overhearPnpm
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
              gst_all_1.gstreamer
              gst_all_1.gst-plugins-base
              gst_all_1.gst-plugins-good
              gst_all_1.gst-plugins-bad
              librsvg
              libayatana-appindicator # トレイアイコン (dlopen される)
            ];

            env = {
              APRIL_LIB_DIR = aprilLibDir;
              # WordNet 3.0 の dict ファイル (英英辞書)。nixpkgs に入っているため
              # 追加のダウンロードは要らない。
              WORDNET_DICT_DIR = "${pkgs.wordnet}/dict";
              WORDNET_JA_PATH = "${pkgs.wordnet-ja}";
              WHISPER_MODEL_PATH = "${pkgs.whisper-model}";
              EJDICT_PATH = "${pkgs.ejdict}/ejdict-hand-utf8.txt";
              IPA_US_PATH = "${pkgs.ipa-us}";
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
