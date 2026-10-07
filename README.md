# overhear

システム音声に常駐する語学学習用の字幕・単語学習アプリ。Netflix でも YouTube でも Spotify でもビデオ会議でも、**音が鳴っていれば効く**。

既存の語学学習ツール (Language Reactor、LLPlayer、asbplayer、SubMiner) は「特定のプレイヤーの中の動画」か「特定のストリーミングサイト」に紐づく。overhear は PipeWire の既定シンクの monitor を掴むため、字幕ファイルの有無にも、サイトの DOM 構造にも、再生元にも依存しない。

## 設計の核

### 1. リングバッファで「聞き直し」と sentence mining が成立する

履歴タブで音声を聞き直せる期間を案内し、各行の「聞き直す」から再生する。保持期間を過ぎた行は「音声なし」と表示して再生を無効にする。音声の保持状態は 5 秒ごとに取得し、更新間隔の間に期限を過ぎた場合は再生時にも案内する。音声認識エンジンやサンプルレートは設定タブの「動作情報」に表示する。

Linux の聞き直しは WebKitGTK と GStreamer で WAV を再生する。Nix パッケージは GStreamer の core / base / good / bad プラグインを起動ラッパーで指定し、ビルド時に再生に必要な要素を検査する。開発用の `nix develop` にも同じプラグインを含める。

音声をキャプチャしている以上、直近 N 分の PCM をメモリに保持できる。字幕行をクリックすればその区間を再生し直せるし、Anki へ「文 + その区間の音声 + 単語 + 訳」を書き出せる。**字幕ファイルなしで mining が成立する**のはこの一点による。

### 2. 即時字幕と任意の精度補正

april-asr が即時字幕を出す。設定の「字幕の精度補正」をオンにすると、発話後にリングバッファの該当区間を whisper.cpp で再認識して字幕の単語や句読点を補正する。CPU 使用量を抑えるため、精度補正の初期値はオフ。選択は次回起動にも引き継ぐ。オフにすると、処理中の再認識と待ち行列を停止し、補正用の子プロセスを終了する。差し替えは **同じ id の segment の更新**として GraphQL subscription に流れるので、フロントは Apollo の正規化キャッシュで受けるだけでよい。

april の出力は全部大文字なので、whisper で差し替わる前の行 (と whisper に掛けない短い行・古い行) は表示だけ小文字にし、文頭と一人称の I を大文字にする。固有名詞は小文字のままになる。辞書は元の語で引く。

### 3. 翻訳も ASR も辞書もストラテジーパターン

既定はローカル (Ollama)。DeepL / Google は必要に応じて選ぶ。`Translator` トレイトは `sends_data_externally` をケイパビリティとして表明するため、UI はローカルと外部送信を分けて見せられる。辞書 (`Dictionary`) と音声認識 (`Recognizer`) も同じ形にしてある。

### 4. 保存した語だけを永続化する

字幕は流れて消える揮発データとして扱い、SQLite に残すのは**保存した語彙だけ**。保存時にその文・訳・語義・音声クリップを焼き付けるので、元の segment が履歴から溢れても、リングバッファから音声が消えても後から復元できる。

## アーキテクチャ

```
PipeWire monitor
  └→ ring buffer (直近 N 分の PCM, 16kHz mono)
       ├→ april-asr (FFI)   … interim segment を即時 publish
       └→ whisper.cpp       … 文確定後に final segment へ差し替え
            └→ 文分割 → 翻訳 (非同期) → 履歴 (メモリ)
                              └→ 語彙として保存 → SQLite + WAV → Anki
                                        ↑
   axum + async-graphql (127.0.0.1, ephemeral port)
      ├ POST /graphql  … Query / Mutation
      ├ WS   /graphql  … Subscription (graphql-ws)
      └ GET  /audio/{segmentId}.wav … 音声の実体 (Range 未対応)
                                        ↑
   Tauri v2 WebView
      React + Tailwind + react-aria-components + Apollo Client
```

ウィンドウは 2 枚ある。

| ウィンドウ           | 役割                                                                                                             |
| -------------------- | ---------------------------------------------------------------------------------------------------------------- |
| 字幕バー (`caption`) | 常時最前面・枠なし・背景透過。動画の上に重ねる。いま話されている文と訳だけを出し、語をクリックすれば辞書は引ける |
| スタジオ (`main`)    | 履歴・語彙・Anki 書き出し・設定                                                                                  |

トレイから表示を切り替えられるほか、グローバルホットキーがある。

| キー         | 動作               |
| ------------ | ------------------ |
| `Ctrl+Alt+O` | 字幕バーの表示切替 |
| `Ctrl+Alt+S` | スタジオの表示切替 |

字幕バーとスタジオの位置と大きさは `$XDG_CONFIG_HOME/com.conao3.overhear/.window-state.json` に残り、次の起動で戻る (`tauri-plugin-window-state`)。プラグインが書くのは正常終了のときだけなので、動かしたり大きさを変えたりしたら 0.5 秒後にも書く。字幕バーは画面下の中央へ置いてから、保存した位置があればそちらへ戻す。字幕バーの表示・非表示は同じディレクトリの `caption-visible.json` に切り替えた直後に保存し、通常起動・自動起動とも前回の状態に戻す。初回は表示する。

`overhear-server` が落ちたら、Tauri が起動し直す。ポートとトークンは起動ごとに変わるので、新しい接続情報を各ウィンドウの `sessionStorage` に入れてページを読み込み直す (初期化スクリプトは `sessionStorage` の値を優先して `window.__OVERHEAR__` にする)。60 秒に 5 回落ちたら起動し直すのをやめる。

2 つ目を起動すると、そちらはすぐ終わり、動いているほうのスタジオが前に出る。`$XDG_RUNTIME_DIR/overhear.lock` を flock で掴めたほうが本体になり、`overhear.sock` で 2 つ目からの知らせを受ける。ホットキーを他のアプリが使っていて登録できないときは、警告を出して起動を続ける (トレイからは操作できる)。

トレイの「ログイン時に起動」で XDG autostart (`$XDG_CONFIG_HOME/autostart/overhear.desktop`) を置き外しする。自動起動では `--autostart` が付き、スタジオを出さずに始まる。字幕バーは前回の表示状態に従う。desktop entry の `Exec` は、パッケージ版では wrapper が渡す `OVERHEAR_AUTOSTART_EXEC` (`overhear`、PATH から引く)、開発中は実行中のバイナリの絶対パスになる。

Tauri が持つのはウィンドウとプロセス管理だけで、ドメインロジックは GraphQL サーバ側にある。Tauri の IPC ではなくローカル HTTP / WebSocket に口を開けているのは、字幕が subscription を本質とするデータであり、Apollo の `GraphQLWsLink` がそのまま使えるため。

ポートは `127.0.0.1` の ephemeral、起動ごとにランダムな 32 文字のトークンを生成し、HTTP は `Authorization: Bearer`、WebSocket は `connectionParams` で要求する。Tauri は接続情報を `window.__OVERHEAR__` でフロントへ渡す。

## 必要なもの

- Linux + PipeWire (`pw-record`)
- april-asr の共有ライブラリとモデル
- whisper.cpp (`whisper-server`) と ggml モデル (精度補正を使う場合)
- WordNet 3.0 (英英辞書) と ejdict-hand (英和辞書)
- Anki + AnkiConnect アドオン (カード書き出しを使う場合のみ)

`libaprilasr.so` は nixpkgs に単体パッケージが無く、**`livecaptions` の出力に同梱**されている。flake の devShell がこれを `APRIL_LIB_DIR` で指し、モデルを `APRIL_MODEL_PATH` に注入する。

WordNet も nixpkgs の `wordnet` に `dict/` が入っているため、追加のダウンロードは要らない (`WORDNET_DICT_DIR`)。英和の ejdict-hand はパブリックドメインのタブ区切りテキストを flake が `fetchzip` で固定する (`EJDICT_PATH`)。

whisper.cpp は nixpkgs の `whisper-cpp` に `whisper-server` が入っており、モデル (`ggml-base.en.bin`) は flake が `fetchurl` で固定する (`WHISPER_MODEL_PATH`)。`whisper-server` は精度補正をオンにしている間だけ子プロセスとして動作し、モデルをメモリに保持する。`--no-whisper` で起動すると精度補正の切り替えを無効にする。

語彙と音声クリップは `$XDG_DATA_HOME/overhear` (既定 `~/.local/share/overhear`) に置く。`OVERHEAR_DATA_DIR` で変えられる。

スタジオで選んだ翻訳エンジンと音源は同じディレクトリの `settings.json` に残り、次の起動で戻る。保存した音源が見つからない (USB の機器を抜いた等) ときは警告を出して既定シンクで起動する。`--translator` を渡した起動では保存値より引数を優先する。

## 入れる

前提は x86_64-linux・PipeWire・flakes が有効な Nix (`experimental-features = nix-command flakes`)。

```sh
nix run github:conao3/rust-overhear        # そのまま起動
nix profile install github:conao3/rust-overhear
```

NixOS / home-manager では flake の入力に足して `packages.default` を入れる。

```nix
# flake.nix
inputs.overhear.url = "github:conao3/rust-overhear";

# home-manager のモジュール
home.packages = [ inputs.overhear.packages.${pkgs.stdenv.hostPlatform.system}.default ];
```

初回はモデル (april 321MB、whisper 142MB) の取得とビルドで数分かかる。翻訳を使うなら別に Ollama を動かしてモデルを pull しておく (home-manager なら `services.ollama.enable = true;` と `ollama pull qwen3:8b`)。DeepL / Google の API キーを設定タブから保存するには Secret Service (gnome-keyring 等) が要る。

`packages.default` はフロント (pnpm) と Rust をまとめてビルドし、april-asr のモデル・whisper のモデル・WordNet・英和辞書のパスと、子プロセスとして使う `pw-record` / `whisper-server` を実行ファイルに焼き込む。devShell の外でもそのまま動く。desktop entry とアイコンも入るのでメニューから起動できる。

## 更新ビルドのキャッシュ

`nix/package.nix` がビルドを分割する。`nix run .` / `nix build .` の使い方は同じ。

- `rust-deps`: Crane で Rust の依存ライブラリとテスト用の成果物をキャッシュする。Cargo の依存・機能・ビルド設定やツールチェーンを変えたときに更新する。
- `frontend`: TypeScript の型検査と Vite のビルド。Rust だけの変更では再利用する。pnpm の依存取得は `package.json` と `pnpm-lock.yaml` の変更で更新する。
- `default`: キャッシュからアプリ本体を release ビルドし、ワークスペースの全テストを実行する。UI は従来どおり実行ファイルに埋め込む。

README などの文書・作業ファイルはビルドの入力から除く。Linux のデスクトップ版では `rlib` と実行ファイルを生成し、使わない `staticlib` / `cdylib` は生成しない。音声認識の最適化レベルは変更していない。

キャッシュが無い初回や依存更新時にはコンパイルが必要。利用中の字幕への負荷を抑えるため、ビルドは引き続き `nix build --option cores 4 --max-jobs 1 .`、更新・再起動は tmux `rust-overhear` セッションで行う。

```sh
python3 scripts/benchmark-build.py          # 文書・Rust・UI・Cargo 設定のキャッシュ境界を検証
python3 scripts/benchmark-build.py --build  # Rust 変更と UI 変更のビルド時間も測定
```

検証用のソースコピーとログはメイン作業ツリーの `.claude-dev/scratch/build-cache/benchmark-*/` に保存する。作業中のコードを変更せず、各ビルドは 4 コア・同時 1 ジョブで実行する。

2026-10-02、同じホスト・4 コアでの各 1 回の実測。変更前はコンパイル 11分41秒 + テスト用ビルドと実行 1分30秒。下表は改善後の `nix build` 起動から完了まで (全 49 テスト・パッケージ生成を含む)。依存キャッシュを事前に作成した状態で測る。

| 変更                                 | 所要時間 |
| ------------------------------------ | -------- |
| Rust のソースに検証用の定数を追加    | 2分26秒  |
| UI の CSS に検証用のプロパティを追加 | 2分45秒  |
| 変更なし、全成果物を再利用           | 6秒      |

測定スクリプトは毎回異なる値を追加し、前回の検証用パッケージがそのまま再利用されるのを避ける。README の変更で全成果物の derivation が不変なことと、Cargo のビルド設定の変更で依存キャッシュが更新されることも検証する。

## 使い方

```sh
nix develop

make dev          # Tauri アプリ (サーバは自動で起動する)
make dev-server   # GraphQL サーバだけを 4747 番で起動 (GraphiQL つき)
make dev-web      # vite だけを 1420 番で起動 (dev-server と組み合わせる)
make test
```

`make dev-server` はトークンを要求しない開発用の起動。`http://127.0.0.1:4747/` に GraphiQL が出る。

```sh
# 直近の発話を見る
curl -s -X POST http://127.0.0.1:4747/graphql \
  -H 'content-type: application/json' \
  -d '{"query":"{ segments(limit: 5) { id status sourceText } }"}'
```

## 翻訳エンジンの設定

| id              | 設定                                                 | 外部送信 |
| --------------- | ---------------------------------------------------- | -------- |
| `ollama` (既定) | `OVERHEAR_OLLAMA_ENDPOINT` / `OVERHEAR_OLLAMA_MODEL` | しない   |
| `deepl`         | 設定タブの API キー (`:fx` で終わる Free キーも可)   | する     |
| `google`        | 設定タブの API キー                                  | する     |
| `none`          | —                                                    | しない   |

### いつ訳すか

**翻訳は全文には掛けない。** 訳すのは次の 2 つのときだけで、どちらも 1 本の worker に積まれ、新しい segment から処理される。

- 字幕バー・スタジオの「訳す」を押したとき
- 台詞が 1.5 秒途切れたとき。最新の 1 行だけを、whisper の差し替えが済んでから訳す。動画を止めると、いま画面にある台詞の訳が出る

台詞が続く動画では、CPU 推論のローカル LLM は全文の翻訳に追いつかない。Core Ultra 5 225U で全文を訳していたときは、台詞が 2 秒に 1 行のペースで確定するのに対し、`num_thread` 4 で overhear 一式が約 6.6 コアを使い続け、確定から訳が付くまで中央値 13 秒、訳が付いたのは 3 割だった。

### 設定タブ

スタジオの「設定」タブで、訳す先の言語・Ollama のモデル (pull 済みのものから選ぶ)・DeepL / Google の API キーを変えられる。言語とモデルはデータディレクトリの `settings.json` に、API キーは Secret Service (gnome-keyring 等) にサービス名 `overhear` で置き、次の起動でも戻る。キーそのものはサーバから返さず、入っているかどうかだけを出す。Secret Service が無い環境でも起動はでき、キーの保存だけがエラーになる。`--target-lang` を渡した起動では保存値より引数を優先する。

スタジオの「翻訳エンジン」で選んだものが翻訳の既定になる。指定エンジンが失敗すると既定エンジンへフォールバックし、`Translation.fallbackFrom` に元のエンジン id が残る。フォールバック先も失敗した場合は翻訳なしで segment を確定させ、字幕そのものは止めない。

### Ollama

Ollama は別に起動しておき (`ollama serve`)、モデルを pull しておく (`ollama pull qwen3:8b`)。サーバは起動時に既定エンジンへ 1 度翻訳を投げ、モデルをメモリへ載せておく。

| 環境変数                     | 既定                     | 意味                                                       |
| ---------------------------- | ------------------------ | ---------------------------------------------------------- |
| `OVERHEAR_OLLAMA_ENDPOINT`   | `http://127.0.0.1:11434` | 接続先                                                     |
| `OVERHEAR_OLLAMA_MODEL`      | `qwen3:8b`               | モデル                                                     |
| `OVERHEAR_OLLAMA_KEEP_ALIVE` | `30m`                    | 最後の翻訳からモデルをメモリに残す時間                     |
| `OVERHEAR_OLLAMA_NUM_THREAD` | Ollama の判断            | 推論スレッド数。動画の再生と取り合うので、全コアは渡さない |

プロンプトには言語コードではなく言語名 (`Japanese`) を渡す。コードのままだと小さいモデルは別の言語で返してくる。

CPU 推論 (Core Ultra 5 225U、`num_thread` 8) で字幕 1 行の英→日にかかった時間:

| モデル              | 1 行 (モデル常駐時) | 品質                                                    |
| ------------------- | ------------------- | ------------------------------------------------------- |
| `qwen3:8b` (既定)   | 約 3.5 秒           | 7 行すべて意味が通る                                    |
| `gemma3:4b`         | 約 2.2 秒           | 否定の取り違えがある                                    |
| `translategemma:4b` | 約 4.3 秒           | `gemma3:4b` と同程度                                    |
| `qwen2.5:3b`        | 約 2.5 秒           | 中国語・英語が混ざる                                    |
| `gemma3:1b` など    | 約 1.3 秒           | 誤訳が多く実用外                                        |
| `qwen3:4b`          | 数百秒              | `think: false` を無視して思考を出力し続けるので使えない |

## 現状

- [x] PipeWire キャプチャ、リングバッファ、april-asr (FFI)
- [x] GraphQL の Query / Mutation / Subscription、トークン認証、音声の WAV 配信
- [x] Tauri + React + Apollo のフロント (キャプションバー、履歴、聞き直し、エンジン選択)
- [x] 翻訳ストラテジー (ollama / deepl / google / none) とフォールバック
- [x] 聞き直しの再生音を拾い直さないミュート (`muteCapture`)
- [x] two-pass ASR。april の即時出力を whisper.cpp の確定文へ差し替える
- [x] 辞書。英和 (ejdict-hand) と英英 (WordNet 3.0) を並べて出す。活用は見出し語へ解く
- [x] 語彙ストア (SQLite)。文・訳・語義・音声を保存時に焼き付ける
- [x] Anki 書き出し (AnkiConnect)。音声つきカードを作る
- [x] 字幕バーとスタジオの 2 ウィンドウ、トレイ、グローバルホットキー
- [x] nix パッケージ化 (`nix run`、desktop entry つき)
- [x] ログイン時の自動起動 (XDG autostart)
- [x] 翻訳エンジンと音源の選択の永続化 (`settings.json`)
- [x] 音源の選択 (再生側の monitor / 録音側)。切り替えは `pw-record` の子プロセスだけを差し替える
- [ ] `pipewire-rs` 直結 (現状は `pw-record` の subprocess)
- [x] API キーの Secret Service (keyring) 保存。スタジオの設定タブから入れる
- [x] 設定タブ (訳す先の言語、Ollama のモデル、API キー)
- [x] キャプチャとサーバが落ちたときの自動再起動
- [x] ウィンドウの位置と大きさの保存

## 即時字幕の遅延

即時の文字起こしを最優先にする。音声取り込み・途中経過の配信・描画に意図的な待ち時間を入れず、辞書や履歴の追加で字幕の更新を重くしない。

- `pw-record` は `--latency 10ms --container raw` で PCM16 を受け取る。読み取りバッファが満杯になるのを待たず、届いた分を認識へ送る。
- 認識モデルの初期化後にキャプチャを始め、初期化中の音声をキューへ溜めない。
- 無音ゲートの余韻は 400ms、語頭を保護する過去音声は 128ms。チャンク数ではなくサンプル数で測るため、音声の到着間隔を短くしても語頭・語尾を切らない。
- 単語の起点はネイティブの button とし、辞書の UI は開いた語だけに作る。変化のない行の単語描画は `memo` で省略する。subscription の受信自体による再描画は抑え、更新されたキャッシュから描画する。

検証では、同じ音声を実時間で ASR に流して最初の字幕と途中経過の受信時刻を比べる。描画は履歴 50 行を置き、最新行だけを毎秒 15 回更新して React Profiler で測る。認識モデルが付ける語の時刻は発話時刻の正解ではないため、それとの差だけをユーザーが感じる遅延として扱わない。

## 単語の辞書

字幕バー、スタジオの字幕、履歴の単語をクリックすると、英和 (ejdict-hand) と英英 (WordNet) の語義を表示する。WordNet が解決した原形も英和辞書で検索するため、`went` から `go`、`running` から `run` の語義を読める。曲がったアポストロフィ (`’`) も辞書検索用に正規化する。

見出し語の下に米語の発音記号 (IPA) を表示する。発音データは [ipa-dict](https://github.com/open-dict-data/ipa-dict) の `en_US` を固定して同梱し、`IPA_US_PATH` から読み込む。検索時の外部通信は発生しない。複数の発音がある語はすべて表示し、未収録の語は語義のみ表示する。活用を解いた場合の発音は、各見出し語に対応する。データの出典とライセンスは `THIRD_PARTY_LICENSES.txt` に記載する。

辞書を開いている間はクリックした字幕を表示し続け、閉じると最新の字幕へ戻る。全語義と例文をスクロールして読め、通信に失敗したときは「再試行」で取得し直せる。「閉じる」ボタン、Escape キー、ポップアップの外側のクリックで閉じられる。

高さ 320px 以下のウィンドウでは、辞書をウィンドウ全体に表示する。通常の高さ 150px の独立字幕バーでも、見出しと閉じるボタンを表示したまま語義をスクロールできる。

スタジオの字幕と履歴では、辞書から語彙を保存できる。保存時に表示中の原文・単語とサーバの字幕を照合し、whisper の補正などで変わっていたら、辞書を開き直すよう案内する。

## Anki への書き出し

Anki を起動し AnkiConnect アドオンを入れておく (`OVERHEAR_ANKI_ENDPOINT`、既定 `http://127.0.0.1:8765`)。語彙タブで選んで「Anki へ書き出す」を押すと、デッキ `overhear` に Basic ノートを作る。

- Front — 見出し語
- Back — 語義 + 保存時の文 + 訳、そこに音声クリップを添付
- タグ — `overhear`

## 負荷

april-asr は供給された音声に対して常に推論を走らせるため、素のままだと**誰も喋っていなくても 1 コアの 6 割前後**を使い続ける (release/debug でほぼ同じ。C ライブラリ側の固定コスト)。常駐アプリとしては重いので、振幅を見て静かな区間は ASR へ供給しない。

| 状態        | CPU (1 コア基準)              |
| ----------- | ----------------------------- |
| 待機 (無音) | 0.0%                          |
| 音声あり    | 供給している間だけ april の分 |

止めたぶん ASR の内部時計は進まなくなるので、ゲートが開くたびに「ASR の時刻 → 絶対時間」の対応点を残し (`ClockMap`)、segment を組むときにトークンごとに足し戻している。april は文の確定を次の音声が来てから出すことが多く、確定時点の差を一律に足すと文間の無音ぶん区間が後ろへずれる。リングバッファには無音も含めて常に入れるため、聞き直しは影響を受けない。`--silence-threshold 0` でゲートを切れる。

whisper の差し替えと翻訳はそれぞれ 1 本の worker で流し、**どちらも新しい segment から処理する**。古い順に処理すると、溜まった分を捌き終えるまで字幕バーの文が差し替わらない。古い segment は台詞が途切れたときに埋まる。翻訳の待ち件数はスタジオのヘッダに「翻訳待ち」として出る。whisper は 600ms 未満の区間には掛けない (相槌や物音が大半で、CPU を使うわりに得るものが無い)。スレッド数の既定は 2。whisper のプロセスは nice 10 で動かし、CPU が競合したときは即時認識を優先する。

april は `ASYNC_NO_RT` で動かす。`ASYNC_RT` は処理が遅れると音声を早回しし、トークンの時刻が早回し後の長さでしか進まなくなるため、CPU が詰まるたびに時刻が実際より遅れて戻らない (長く動かすほど whisper に渡す区間がずれていく)。処理が追いつかずに april が音声を捨てたときは、その長さを飛ばしたのと同じに数えて時刻を戻す。途中経過は受信直後に配信し、時間による間引きは行わない。30 秒より古い台詞は whisper に掛けない。

whisper に渡す区間は、april のトークン時刻から前 200ms・後ろ 500ms の余白を取り、前後の segment に食い込む分を削る。april のトークン時刻は語を認識し終えた時点のもので、実際の発話より 0.3〜0.6 秒遅い (whisper-cli の語単位タイムスタンプとの比較)。そのため 400ms 引いてから使う。

台詞が途切れない動画では whisper の worker が動き続け、約 2.6 コア (whisper 1.9 + april を含むサーバ 0.7) を使う。翻訳は求められた行と途切れた時の最新行だけなので、ollama が動くのはその間だけになる。

`pw-record` が終了したら (既定のデバイスを抜いた、PipeWire が再起動した等) 1 秒後に起動し直し、失敗が続けば 30 秒まで間隔を延ばして試し続ける。止まっている間はスタジオのヘッダに出る。

子プロセス (`pw-record` / `whisper-server` / `overhear-server`) は `PR_SET_PDEATHSIG` で親と一緒に落ちる。親が SIGKILL されると Drop が走らないため、これが無いと ASR を抱えたプロセスが孤児として残り、1 コアずつ食い続ける。

## 既知の制約

- **聞き直しの再生音は既定シンクの monitor に戻ってくる。** フロントは再生の前に `muteCapture` を呼び、その間の入力を無音に差し替えている (破棄ではなく無音なのは ASR とリングバッファの時間軸を止めないため)。裏返しとして、**再生中は実際の音声が書き起こされない**
- `/audio/{id}.wav` は Range 未対応。数秒のクリップ前提で全体を返す
- whisper は語ごとの時刻を返さないため、差し替え後のトークンの時刻は segment の区間に均等割りしている
- 字幕バーの背景透過はコンポジットが有効な環境でのみ効く。無効なら単に不透明になる (壊れはしない)
- 字幕バーとスタジオは別の HTML (`caption.html` / `index.html`)。`?window=caption` のようなクエリでの振り分けは vite の dev サーバでは通るが、配布ビルドのアセット解決では失敗する
- パッケージ版の WM class は wrapper 由来の名前 (`.overhear-wrapped`) になる。argv0 を変えても追従しないため、desktop entry に `StartupWMClass` は入れていない。ランチャのアイコンとウィンドウが紐づかないだけで、起動と動作には影響しない
- whisper は `[MUSIC PLAYING]` のような非発話マーカーを返す。発話に混ざっている場合だけ取り除き、区間全体がマーカーのときは台詞が無いことを示すために残す (翻訳には回さない)
- 台詞が続いている間は訳が出ない (途切れるか「訳す」を押したときに出る)。CPU 推論のローカル翻訳が全文に追いつかないため
- april-asr は英語モデルのみ。話者分離は無い

## ライセンス

GPL-3.0-only。april-asr が GPL-3.0 であり、`libaprilasr.so` をリンクするため。

## 検証ツール

認証付き字幕の購読・集計と AnkiConnect モックの使い方は [scripts/README.md](scripts/README.md) を参照する。
