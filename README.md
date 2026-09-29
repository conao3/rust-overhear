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
       └→ whisper.cpp       … 文確定後に final segment へ差し替え (未実装)
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

Tauri が持つのはウィンドウとプロセス管理だけで、ドメインロジックは GraphQL サーバ側にある。Tauri の IPC ではなくローカル HTTP / WebSocket に口を開けているのは、字幕が subscription を本質とするデータであり、Apollo の `GraphQLWsLink` がそのまま使えるため。

ポートは `127.0.0.1` の ephemeral、起動ごとにランダムな 32 文字のトークンを生成し、HTTP は `Authorization: Bearer`、WebSocket は `connectionParams` で要求する。Tauri は接続情報を `window.__OVERHEAR__` でフロントへ渡す。

## 必要なもの

- Linux + PipeWire (`pw-record`)
- april-asr の共有ライブラリとモデル
- WordNet 3.0 (英英辞書)
- Anki + AnkiConnect アドオン (カード書き出しを使う場合のみ)

`libaprilasr.so` は nixpkgs に単体パッケージが無く、**`livecaptions` の出力に同梱**されている。flake の devShell がこれを `APRIL_LIB_DIR` で指し、モデルを `APRIL_MODEL_PATH` に注入する。

WordNet も nixpkgs の `wordnet` に `dict/` が入っているため、追加のダウンロードは要らない (`WORDNET_DICT_DIR`)。

語彙と音声クリップは `$XDG_DATA_HOME/overhear` (既定 `~/.local/share/overhear`) に置く。`OVERHEAR_DATA_DIR` で変えられる。

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

## 現状

- [x] PipeWire キャプチャ、リングバッファ、april-asr (FFI)
- [x] GraphQL の Query / Mutation / Subscription、トークン認証、音声の WAV 配信
- [x] Tauri + React + Apollo のフロント (キャプションバー、履歴、聞き直し、エンジン選択)
- [x] 翻訳ストラテジー (ollama / deepl / google / none) とフォールバック
- [x] 聞き直しの再生音を拾い直さないミュート (`muteCapture`)
- [x] 辞書 (WordNet 3.0)。活用は見出し語へ解く
- [x] 語彙ストア (SQLite)。文・訳・語義・音声を保存時に焼き付ける
- [x] Anki 書き出し (AnkiConnect)。音声つきカードを作る
- [ ] whisper.cpp による確定文への差し替え (two-pass の後段)
- [ ] 英和辞書 (現状は英英のみ)
- [ ] `pipewire-rs` 直結、デバイス選択、グローバルホットキー、nix パッケージ化
- [ ] API キーの Secret Service (keyring) 保存 (現状は環境変数)

## Anki への書き出し

Anki を起動し AnkiConnect アドオンを入れておく (`OVERHEAR_ANKI_ENDPOINT`、既定 `http://127.0.0.1:8765`)。語彙タブで選んで「Anki へ書き出す」を押すと、デッキ `overhear` に Basic ノートを作る。

- Front — 見出し語
- Back — 語義 + 保存時の文 + 訳、そこに音声クリップを添付
- タグ — `overhear`

## 既知の制約

- **聞き直しの再生音は既定シンクの monitor に戻ってくる。** フロントは再生の前に `muteCapture` を呼び、その間の入力を無音に差し替えている (破棄ではなく無音なのは ASR とリングバッファの時間軸を止めないため)。裏返しとして、**再生中は実際の音声が書き起こされない**
- `/audio/{id}.wav` は Range 未対応。数秒のクリップ前提で全体を返す
- april-asr は英語モデルのみ。話者分離は無い
- API キーは環境変数から読む (keyring 未対応)

## ライセンス

GPL-3.0-only。april-asr が GPL-3.0 であり、`libaprilasr.so` をリンクするため。
