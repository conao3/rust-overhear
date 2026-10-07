# 検証スクリプト

`nix develop` の環境で実行する。結果はメイン worktree の `.claude-dev/scratch/` 配下へ保存する。

## 字幕の購読と集計

`overhear-server` が標準出力の先頭に出す接続情報 JSON をファイルへ保存し、認証付きで購読する。接続情報にはトークンが含まれるため、ファイルを手元で管理する。

```sh
node scripts/record-segments.mjs --connection .claude-dev/scratch/connection.json --output .claude-dev/scratch/segments.jsonl --seconds 30
python3 scripts/summarize-segments.py .claude-dev/scratch/segments.jsonl
```

記録するのは受信時刻 (開始からのミリ秒)、字幕 ID、認識状態・エンジン、原文、音声区間、語、翻訳。指定時間の経過で終了し、認証エラー・通信エラー・受信 0 件は失敗とする。出力先は新規ファイルを指定する。

## AnkiConnect のモック

```sh
python3 scripts/mock-anki.py --port 8799 --output .claude-dev/scratch/anki-requests.jsonl
```

起動時に接続先を出力する。`--port 0` は空きポートを割り当てる。検証用サーバの `OVERHEAR_ANKI_ENDPOINT` をこの接続先に設定する。

`createDeck` はデッキ ID、`addNote` は呼び出しごとに異なるノート ID を返す。その他の action はエラー応答にする。送った JSON は 1 行ずつ出力ファイルへ記録される。終了は Ctrl-C。

## ビルドの計測

`python3 scripts/benchmark-build.py` でキャッシュ境界を検証する。`--build` を付けるとビルド時間も測る。
