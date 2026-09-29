# overhear

システム音声に常駐する語学学習用の字幕・単語学習アプリ。Netflix でも YouTube でも Spotify でもビデオ会議でも、**音が鳴っていれば効く**。

既存の語学学習ツール (Language Reactor、LLPlayer、asbplayer、SubMiner) は「特定のプレイヤーの中の動画」か「特定のストリーミングサイト」に紐づく。overhear は PipeWire の既定シンクの monitor を掴むため、字幕ファイルの有無にも、サイトの DOM 構造にも、再生元にも依存しない。

## 設計の核

### 1. リングバッファで「聞き直し」と sentence mining が成立する

音声をキャプチャしている以上、直近 N 分の PCM をメモリに保持できる。字幕行をクリックすればその区間を再生し直せるし、Anki へ「文 + その区間の音声 + 単語 + 訳」を書き出せる。**字幕ファイルなしで mining が成立する**のはこの一点による。

### 2. two-pass ASR で「即応性」と「読める文」を両立する

april-asr が interim を即座に出し、文が閉じたらリングバッファの該当区間を whisper.cpp に投げて確定文へ差し替える。差し替えは **同じ id の segment の更新**として GraphQL subscription に流れるので、フロントは Apollo の正規化キャッシュで受けるだけでよい。

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

Tauri が持つのはウィンドウとプロセス管理だけで、ドメインロジックは GraphQL サーバ側にある。Tauri の IPC ではなくローカル HTTP / WebSocket に口を開けているのは、字幕が subscription を本質とするデータであり、Apollo の `GraphQLWsLink` がそのまま使えるため。

ポートは `127.0.0.1` の ephemeral、起動ごとにランダムな 32 文字のトークンを生成し、HTTP は `Authorization: Bearer`、WebSocket は `connectionParams` で要求する。Tauri は接続情報を `window.__OVERHEAR__` でフロントへ渡す。

## 必要なもの

- Linux + PipeWire (`pw-record`)
- april-asr の共有ライブラリとモデル
- whisper.cpp (`whisper-server`) と ggml モデル
- WordNet 3.0 (英英辞書) と ejdict-hand (英和辞書)
- Anki + AnkiConnect アドオン (カード書き出しを使う場合のみ)

`libaprilasr.so` は nixpkgs に単体パッケージが無く、**`livecaptions` の出力に同梱**されている。flake の devShell がこれを `APRIL_LIB_DIR` で指し、モデルを `APRIL_MODEL_PATH` に注入する。

WordNet も nixpkgs の `wordnet` に `dict/` が入っているため、追加のダウンロードは要らない (`WORDNET_DICT_DIR`)。英和の ejdict-hand はパブリックドメインのタブ区切りテキストを flake が `fetchzip` で固定する (`EJDICT_PATH`)。

whisper.cpp は nixpkgs の `whisper-cpp` に `whisper-server` が入っており、モデル (`ggml-base.en.bin`) は flake が `fetchurl` で固定する (`WHISPER_MODEL_PATH`)。`whisper-server` は子プロセスとして 1 度だけ起動するので、モデルの読み込みは 1 回で済む。`--no-whisper` で後段を切れる。

語彙と音声クリップは `$XDG_DATA_HOME/overhear` (既定 `~/.local/share/overhear`) に置く。`OVERHEAR_DATA_DIR` で変えられる。

## 入れる

```sh
nix run github:conao3/rust-overhear        # そのまま起動
nix profile install github:conao3/rust-overhear
```

`packages.default` はフロント (pnpm) と Rust をまとめてビルドし、april-asr のモデル・whisper のモデル・WordNet・英和辞書のパスと、子プロセスとして使う `pw-record` / `whisper-server` を実行ファイルに焼き込む。devShell の外でもそのまま動く。desktop entry とアイコンも入るのでメニューから起動できる。

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
| `deepl`         | `OVERHEAR_DEEPL_API_KEY`                             | する     |
| `google`        | `OVERHEAR_GOOGLE_API_KEY`                            | する     |
| `none`          | —                                                    | しない   |

指定エンジンが失敗すると既定エンジンへフォールバックし、`Translation.fallbackFrom` に元のエンジン id が残る。フォールバック先も失敗した場合は翻訳なしで segment を確定させ、字幕そのものは止めない。

### Ollama

Ollama は別に起動しておき (`ollama serve`)、モデルを pull しておく (`ollama pull qwen3:8b`)。サーバは起動時に既定エンジンへ 1 度翻訳を投げ、モデルをメモリへ載せておく。

| 環境変数                     | 既定                     | 意味                                                     |
| ---------------------------- | ------------------------ | -------------------------------------------------------- |
| `OVERHEAR_OLLAMA_ENDPOINT`   | `http://127.0.0.1:11434` | 接続先                                                   |
| `OVERHEAR_OLLAMA_MODEL`      | `qwen3:8b`               | モデル                                                   |
| `OVERHEAR_OLLAMA_KEEP_ALIVE` | `30m`                    | 最後の翻訳からモデルをメモリに残す時間                   |
| `OVERHEAR_OLLAMA_NUM_THREAD` | Ollama の判断            | 推論スレッド数。CPU 推論では明示したほうが速いことが多い |

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
- [x] 音源の選択 (再生側の monitor / 録音側)。切り替えは `pw-record` の子プロセスだけを差し替える
- [ ] `pipewire-rs` 直結 (現状は `pw-record` の subprocess)
- [ ] API キーの Secret Service (keyring) 保存 (現状は環境変数)

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

止めたぶん ASR の内部時計は進まなくなるので、差を `skipped_samples` で持ち、segment を組むときに足し戻している。リングバッファには無音も含めて常に入れるため、聞き直しは影響を受けない。`--silence-threshold 0` でゲートを切れる。

whisper の差し替えは同時に 1 本までに絞り、600ms 未満の区間には掛けない (相槌や物音が大半で、CPU を使うわりに得るものが無い)。スレッド数の既定は 2。

翻訳も 1 本ずつ流し、**新しい segment から訳す**。ローカル LLM の翻訳は 1 行に数秒かかり、発話が続くと確定の間隔より遅くなるため、古い順に訳すと字幕バーの訳が何行も遅れる。古い segment は発話が途切れたときに埋まる。待ち件数はスタジオのヘッダに「翻訳待ち」として出る。

子プロセス (`pw-record` / `whisper-server` / `overhear-server`) は `PR_SET_PDEATHSIG` で親と一緒に落ちる。親が SIGKILL されると Drop が走らないため、これが無いと ASR を抱えたプロセスが孤児として残り、1 コアずつ食い続ける。

## 既知の制約

- **聞き直しの再生音は既定シンクの monitor に戻ってくる。** フロントは再生の前に `muteCapture` を呼び、その間の入力を無音に差し替えている (破棄ではなく無音なのは ASR とリングバッファの時間軸を止めないため)。裏返しとして、**再生中は実際の音声が書き起こされない**
- `/audio/{id}.wav` は Range 未対応。数秒のクリップ前提で全体を返す
- whisper は語ごとの時刻を返さないため、差し替え後のトークンの時刻は segment の区間に均等割りしている
- 字幕バーの背景透過はコンポジットが有効な環境でのみ効く。無効なら単に不透明になる (壊れはしない)
- 字幕バーとスタジオは別の HTML (`caption.html` / `index.html`)。`?window=caption` のようなクエリでの振り分けは vite の dev サーバでは通るが、配布ビルドのアセット解決では失敗する
- パッケージ版の WM class は wrapper 由来の名前 (`.overhear-wrapped`) になる。argv0 を変えても追従しないため、desktop entry に `StartupWMClass` は入れていない。ランチャのアイコンとウィンドウが紐づかないだけで、起動と動作には影響しない
- whisper は `[MUSIC PLAYING]` のような非発話マーカーを返す。発話に混ざっている場合だけ取り除き、区間全体がマーカーのときは台詞が無いことを示すために残す
- april-asr は英語モデルのみ。話者分離は無い
- API キーは環境変数から読む (keyring 未対応)

## ライセンス

GPL-3.0-only。april-asr が GPL-3.0 であり、`libaprilasr.so` をリンクするため。
