//! PDF 导出（任务书 §6「数据导出」）。
//!
//! ## 为什么不用纯 Rust 的 PDF 库
//!
//! 「中文可读」是硬要求。核实过三条路线：
//!
//! 1. `printpdf` 之类的库内置字体是 PDF 标准字体（WinAnsi 单字节编码），
//!    中文放不进去；官方 issue 明确说非 Latin 字符会被**静默丢弃**，
//!    要自嵌 TTF/OTF，而 CJK 嵌入有多个未修复的字形回归。
//! 2. `typst` 排版质量最好，但要多带一套中文字体（其内置字体不含中文），
//!    安装包会明显变大，且字体许可需要单独处理。
//! 3. **应用本身就是 WebView2（Chromium 内核）**，直接调用它自己的
//!    `PrintToPdf`：中文由系统字体渲染，所见即所得，零额外依赖。
//!
//! 选 3。代价是导出期间界面要切到"打印视图"（由前端负责切换），
//! 因为 `PrintToPdf` 打印的是**当前页面**。
//!
//! ## 线程模型（容易踩坑的地方）
//!
//! - `with_webview` 的闭包在**主线程**执行；
//! - `PrintToPdf` 的完成回调也由主线程的消息泵派发；
//! - 因此**不能在主线程等待结果**，否则回调永远送不到 → 死锁。
//!
//! 所以：`with_webview` 只负责发起调用，结果通过 oneshot channel 送回
//! 异步运行时，命令在 `rx.await` 上等待。

use crate::error::{AppError, AppResult};
use tauri::{AppHandle, Manager};

/// 把主窗口当前页面导出为 PDF。
///
/// `path` 由前端通过保存对话框取得（用户明确选择的位置）。
#[tauri::command]
pub async fn export_pdf(app: AppHandle, path: String) -> AppResult<()> {
    if path.trim().is_empty() {
        return Err(AppError::validation("导出路径不能为空"));
    }
    // 只接受 .pdf：避免用户把 PDF 内容写成语义不符的文件名
    if !path.to_lowercase().ends_with(".pdf") {
        return Err(AppError::validation("导出文件必须以 .pdf 结尾")
            .with_hint("请选择「PDF 文件（*.pdf）」类型后再保存"));
    }

    #[cfg(windows)]
    {
        windows_impl::print_to_pdf(app, path).await
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        Err(AppError::new(
            crate::error::ErrorCode::Internal,
            "当前平台暂不支持直接导出 PDF",
        )
        .with_hint("请在 Windows 上使用该功能，或改用数据备份导出"))
    }
}

#[cfg(windows)]
mod windows_impl {
    use super::*;
    use std::sync::{Arc, Mutex};
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2Environment6, ICoreWebView2PrintSettings, ICoreWebView2_7,
    };
    use webview2_com::PrintToPdfCompletedHandler;
    use windows::core::{Interface, PCWSTR};

    /// 执行导出。A4 纸张 + 8mm 边距 + 打印背景色（否则表格底色会丢）。
    pub(super) async fn print_to_pdf(app: AppHandle, path: String) -> AppResult<()> {
        let window = app.get_webview_window(crate::MAIN_WINDOW).ok_or_else(|| {
            AppError::new(
                crate::error::ErrorCode::Internal,
                "找不到主窗口，无法导出 PDF",
            )
        })?;

        // oneshot 把完成回调的结果送回 async 上下文
        let (tx, rx) = tokio::sync::oneshot::channel::<Result<(), String>>();
        let tx = Arc::new(Mutex::new(Some(tx)));

        // WebView2 要的是以 0 结尾的宽字符串
        let mut wide: Vec<u16> = path.encode_utf16().collect();
        wide.push(0);

        let tx_outer = tx.clone();
        window
            .with_webview(move |pw| {
                let outcome = (|| -> windows::core::Result<()> {
                    let controller = pw.controller();
                    // SAFETY: 在 WebView2 自己的主线程上调用，接口有效
                    let core = unsafe { controller.CoreWebView2()? };
                    let webview: ICoreWebView2_7 = core.cast()?;

                    // 打印设置：A4（英寸）、8mm 边距、打印背景
                    // 取不到设置不算失败——传 None 用 WebView2 默认值也能出 PDF
                    let settings: Option<ICoreWebView2PrintSettings> = (|| {
                        let env = pw.environment();
                        let env6: ICoreWebView2Environment6 = env.cast().ok()?;
                        let s = unsafe { env6.CreatePrintSettings().ok()? };
                        unsafe {
                            let _ = s.SetPageWidth(8.27);
                            let _ = s.SetPageHeight(11.69);
                            let _ = s.SetMarginTop(0.31);
                            let _ = s.SetMarginBottom(0.31);
                            let _ = s.SetMarginLeft(0.31);
                            let _ = s.SetMarginRight(0.31);
                            let _ = s.SetShouldPrintBackgrounds(true);
                        }
                        Some(s)
                    })();

                    let tx_cb = tx_outer.clone();
                    let handler = PrintToPdfCompletedHandler::create(Box::new(
                        move |hr: windows::core::Result<()>, is_successful: bool| {
                            if let Some(tx) = tx_cb.lock().unwrap().take() {
                                let _ = if hr.is_ok() && is_successful {
                                    tx.send(Ok(()))
                                } else {
                                    tx.send(Err(format!(
                                        "WebView2 导出失败（HRESULT {:?}，success={is_successful}）",
                                        hr
                                    )))
                                };
                            }
                            Ok(())
                        },
                    ));

                    let path_ptr = PCWSTR(wide.as_ptr());
                    // printsettings 传 None 表示用 WebView2 的默认设置
                    unsafe { webview.PrintToPdf(path_ptr, settings.as_ref(), &handler)? };
                    Ok(())
                })();

                // 发起阶段就失败时，回调永远不会被调用，必须自己把错误送回去
                if let Err(e) = outcome {
                    if let Some(tx) = tx_outer.lock().unwrap().take() {
                        let _ = tx.send(Err(format!("发起 PDF 导出失败：{e}")));
                    }
                }
            })
            .map_err(|e| {
                AppError::new(
                    crate::error::ErrorCode::Internal,
                    format!("无法访问主窗口的 WebView：{e}"),
                )
            })?;

        match rx.await {
            Ok(Ok(())) => {
                log::info!("已导出 PDF");
                Ok(())
            }
            Ok(Err(msg)) => Err(AppError::new(crate::error::ErrorCode::Internal, msg)
                .with_hint("请确认目标文件没有被其它程序占用后重试")),
            Err(_) => Err(
                AppError::new(crate::error::ErrorCode::Internal, "PDF 导出没有返回结果")
                    .with_hint("请重试一次；若持续失败，可先用「数据备份」导出 JSON"),
            ),
        }
    }
}
