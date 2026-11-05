use clap::{Args, Parser, Subcommand};
use overhear_cli::GlobalArgs;

#[derive(Parser, Debug)]
#[command(
    name = "overhear",
    version,
    about = "システム音声を字幕にする語学学習アプリ。引数なしで起動するとアプリを開く",
    subcommand_required = true
)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// デスクトップアプリ
    #[command(subcommand)]
    App(AppCommand),
}

#[derive(Subcommand, Debug)]
pub enum AppCommand {
    /// 字幕バーとスタジオを開く。動いているものがあれば、そのスタジオを前に出して終わる
    StartApp(StartAppArgs),
}

#[derive(Args, Debug, Clone)]
pub struct StartAppArgs {
    /// ログイン時の自動起動として始める。スタジオを出さない。
    #[arg(long, overrides_with = "no_autostart")]
    pub autostart: bool,
    /// スタジオを出して始める (既定)。
    #[arg(long, overrides_with = "autostart")]
    pub no_autostart: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use overhear_cli::{EXIT_PARAM, Parsed, parse};
    use std::ffi::OsString;

    fn argv(args: &[&str]) -> Vec<OsString> {
        std::iter::once("overhear")
            .chain(args.iter().copied())
            .map(OsString::from)
            .collect()
    }

    #[test]
    fn parses_autostart() {
        let Parsed::Run(cli) = parse::<Cli>(argv(&["app", "start-app", "--autostart"])) else {
            panic!("expected run");
        };
        let Command::App(AppCommand::StartApp(args)) = cli.command;
        assert!(args.autostart);
    }

    #[test]
    fn rejects_the_flat_flag() {
        let Parsed::Exit(code) = parse::<Cli>(argv(&["--autostart"])) else {
            panic!("expected exit");
        };
        assert_eq!(code, EXIT_PARAM);
    }
}
