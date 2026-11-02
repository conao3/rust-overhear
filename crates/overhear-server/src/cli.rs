use clap::{Args, Parser, Subcommand};
use overhear_cli::{CliSpec, GlobalArgs, InputArgs, parse_integer, parse_string};
use serde::Serialize;
use serde_json::Value;

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
    #[command(flatten)]
    pub input: InputArgs,
}

#[derive(Serialize, Default, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct StartServerOutput {
    pub base_url: String,
    pub graphql_url: String,
    pub websocket_url: String,
    pub token: Option<String>,
}

impl CliSpec for Cli {
    fn global(&self) -> &GlobalArgs {
        &self.global
    }

    fn input(&self) -> &InputArgs {
        match &self.command {
            Command::Server(ServerCommand::StartServer(args)) => &args.input,
        }
    }

    fn output_skeleton(&self) -> Value {
        let output = match &self.command {
            Command::Server(ServerCommand::StartServer(_)) => StartServerOutput {
                token: Some(String::new()),
                ..StartServerOutput::default()
            },
        };
        serde_json::to_value(output).expect("output skeleton serializes")
    }
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

    fn start_server(args: &[&str]) -> (StartServerArgs, Option<Value>) {
        match parse::<Cli>(argv(args)) {
            Parsed::Run(cli, skeleton) => {
                let Command::Server(ServerCommand::StartServer(args)) = cli.command;
                (args, skeleton)
            }
            Parsed::Exit(code) => panic!("exited with {code}"),
        }
    }

    #[test]
    fn parses_dev_server_flags() {
        let (args, _) = start_server(&[
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
    fn skeletons_use_pascal_case() {
        let (_, input) = start_server(&["server", "start-server", "--generate-cli-skeleton"]);
        let input = input.unwrap();
        for key in [
            "Port",
            "Auth",
            "Graphiql",
            "Mock",
            "Whisper",
            "RingSeconds",
            "TargetLang",
        ] {
            assert!(input.get(key).is_some(), "{key}");
        }
        let (_, output) = start_server(&[
            "server",
            "start-server",
            "--generate-cli-skeleton",
            "output",
        ]);
        assert_eq!(
            output.unwrap(),
            serde_json::json!({"BaseUrl": "", "GraphqlUrl": "", "WebsocketUrl": "", "Token": ""})
        );
    }
}
