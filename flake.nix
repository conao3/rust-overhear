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
              pnpm = prev.pnpm_10.override { inherit nodejs; };
              rustToolchain = prev.rust-bin.stable.latest.default;

              # april-asr の学習済みモデル。livecaptions の overlay
              # (nixos-configuration) と同じ URL / hash を使うため、既に store に
              # あるものが再利用され再ダウンロードは発生しない。
              april-model = final.fetchurl {
                url = "https://april.sapples.net/april-english-dev-01110_en.april";
                hash = "sha256-d+uV0PpPdwijfoaMImUwHubELcsl5jymPuo9nLrbwfM=";
              };
            in
            {
              inherit
                nodejs
                pnpm
                rustToolchain
                april-model
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
        in
        {
          devShells.default = pkgs.mkShell {
            packages = with pkgs; [
              rustToolchain
              nodejs
              pnpm
              pkg-config
              pipewire # pw-record (音声キャプチャ)
              sqlite
              wordnet
            ];

            buildInputs = with pkgs; [
              openssl
              sqlite
              # Tauri (Linux)
              webkitgtk_4_1
              gtk3
              libsoup_3
              glib-networking
              librsvg
            ];

            env = {
              APRIL_LIB_DIR = aprilLibDir;
              # WordNet 3.0 の dict ファイル (英英辞書)。nixpkgs に入っているため
              # 追加のダウンロードは要らない。
              WORDNET_DICT_DIR = "${pkgs.wordnet}/dict";
              APRIL_MODEL_PATH = "${pkgs.april-model}";
              LD_LIBRARY_PATH = aprilLibDir;
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
