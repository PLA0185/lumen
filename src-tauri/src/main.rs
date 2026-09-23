#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// 入口仅负责调用库中的 run()。
// 真正的逻辑放 lib.rs，便于被测试与后续拆分模块（任务书 §2.4 功能分模块开发）。
fn main() {
    aitodo_lib::run()
}
