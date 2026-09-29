// Windows の release ビルドでコンソールを出さない
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    overhear_lib::run()
}
