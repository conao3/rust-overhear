// Windows の release ビルドでコンソールを出さない
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    std::process::exit(overhear_lib::main(std::env::args_os().collect()))
}
