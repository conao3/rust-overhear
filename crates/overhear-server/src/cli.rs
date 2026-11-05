use clap::{Args, Parser, Subcommand};
use overhear_cli::{GlobalArgs, parse_integer, parse_string};
use serde::Serialize;

#[derive(Parser, Debug)]
#[command(
    name = "overhear-server",
    version,
    about = "システム音声の字幕化 GraphQL サーバ",
    subcommand_required = true,
    arg_required_else_help = true
)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// GraphQL サーバ
    #[command(subcommand)]
    Server(ServerCommand),
}

#[derive(Subcommand, Debug)]
pub enum ServerCommand {
    /// サーバを起動し、接続情報を出力して待ち受け続ける
    StartServer(StartServerArgs),
}

#[derive(Args, Debug, Clone)]
pub struct StartServerArgs {
    /// 待ち受けポート。省くか 0 なら OS に任せる。
    #[arg(long, value_name = "INTEGER", value_parser = parse_integer::<u16>)]
    pub port: Option<u16>,
    /// 認証トークンを要求する (既定)。
    #[arg(long, overrides_with = "no_auth")]
    pub auth: bool,
    /// 認証トークンを要求しない (開発用)。
    #[arg(long, overrides_with = "auth")]
    pub no_auth: bool,
    /// GraphiQL を / に出す (開発用)。
    #[arg(long, overrides_with = "no_graphiql")]
    pub graphiql: bool,
    /// GraphiQL を出さない (既定)。
    #[arg(long, overrides_with = "graphiql")]
    pub no_graphiql: bool,
    /// 擬似 ASR で動かす。april のモデルが無くても起動できる。
    #[arg(long, overrides_with = "no_mock")]
    pub mock: bool,
    /// april-asr で動かす (既定)。
    #[arg(long, overrides_with = "mock")]
    pub no_mock: bool,
    /// two-pass ASR の後段 (whisper.cpp) を使えるようにする (既定)。
    #[arg(long, overrides_with = "no_whisper")]
    pub whisper: bool,
    /// two-pass ASR の後段 (whisper.cpp) を使わない。
    #[arg(long, overrides_with = "whisper")]
    pub no_whisper: bool,
    /// whisper-server に渡すスレッド数 (既定 2)。後追いの処理なので控えめにしてある。
    #[arg(long, value_name = "INTEGER", value_parser = parse_integer::<usize>)]
    pub whisper_threads: Option<usize>,
    /// 無音とみなす振幅のしきい値。0 で無音ゲートを切る。
    #[arg(long, value_name = "INTEGER", value_parser = parse_integer::<u16>)]
    pub silence_threshold: Option<u16>,
    /// リングバッファの長さ (秒、既定 600)。
    #[arg(long, value_name = "INTEGER", value_parser = parse_integer::<usize>)]
    pub ring_seconds: Option<usize>,
    /// 翻訳先の言語。省くと前回 UI で選んだもの、それも無ければ ja。
    #[arg(long, value_name = "STRING", value_parser = parse_string)]
    pub target_lang: Option<String>,
    /// 既定の翻訳エンジン。省くと前回 UI で選んだもの、それも無ければ ollama。
    #[arg(long, value_name = "STRING", value_parser = parse_string)]
    pub translator: Option<String>,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct StartServerOutput {
    pub base_url: String,
    pub graphql_url: String,
    pub websocket_url: String,
    pub token: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use overhear_cli::{EXIT_PARAM, Parsed, parse};
    use std::ffi::OsString;

    fn argv(args: &[&str]) -> Vec<OsString> {
        std::iter::once("overhear-server")
            .chain(args.iter().copied())
            .map(OsString::from)
            .collect()
    }

    fn start_server(args: &[&str]) -> StartServerArgs {
        match parse::<Cli>(argv(args)) {
            Parsed::Run(cli) => {
                let Command::Server(ServerCommand::StartServer(args)) = cli.command;
                args
            }
            Parsed::Exit(code) => panic!("exited with {code}"),
        }
    }

    #[test]
    fn parses_dev_server_flags() {
        let args = start_server(&[
            "server",
            "start-server",
            "--port",
            "4747",
            "--no-auth",
            "--graphiql",
            "--no-whisper",
        ]);
        assert_eq!(args.port, Some(4747));
        assert!(args.no_auth);
        assert!(args.graphiql);
        assert!(args.no_whisper);
        assert!(!args.mock);
    }

    #[test]
    fn rejects_the_flat_syntax() {
        let Parsed::Exit(code) = parse::<Cli>(argv(&["--port", "4747"])) else {
            panic!("expected exit");
        };
        assert_eq!(code, EXIT_PARAM);
    }

    #[test]
    fn output_keys_are_pascal_case() {
        let output = StartServerOutput {
            base_url: "http://127.0.0.1:4747".into(),
            graphql_url: "http://127.0.0.1:4747/graphql".into(),
            websocket_url: "ws://127.0.0.1:4747/graphql".into(),
            token: Some("t".into()),
        };
        assert_eq!(
            serde_json::to_value(output).unwrap(),
            serde_json::json!({
                "BaseUrl": "http://127.0.0.1:4747",
                "GraphqlUrl": "http://127.0.0.1:4747/graphql",
                "WebsocketUrl": "ws://127.0.0.1:4747/graphql",
                "Token": "t"
            })
        );
    }
}
