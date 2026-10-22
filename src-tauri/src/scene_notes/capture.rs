//! Foreground-window capture only. Frames are returned as transient memory and are never written
//! to disk by this module.

use crate::error::{AppError, AppResult};

#[cfg(windows)]
use std::{io::Cursor, sync::mpsc, time::Duration};

#[cfg(windows)]
use windows_capture::{
    capture::{Context, GraphicsCaptureApiHandler},
    frame::Frame,
    graphics_capture_api::InternalCaptureControl,
    settings::{
        ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
        MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
    },
    window::Window,
};

#[derive(Debug)]
pub struct ForegroundFrame {
    pub process_name: String,
    pub window_title: String,
    pub width: u32,
    pub height: u32,
    pub png: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct ForegroundWindowInfo {
    pub process_name: String,
    pub window_title: String,
}

#[cfg(windows)]
pub fn foreground_info() -> AppResult<ForegroundWindowInfo> {
    let window = Window::foreground().map_err(|_| AppError::validation("没有可采集的前台窗口"))?;
    foreground_info_for(&window)
}

#[cfg(windows)]
fn foreground_info_for(window: &Window) -> AppResult<ForegroundWindowInfo> {
    if !window.is_valid() {
        return Err(AppError::validation("当前前台窗口不能作为采集目标"));
    }
    let process_name = window
        .process_name()
        .map_err(|_| AppError::validation("无法确认前台窗口所属程序；本次未采集画面"))?;
    let current_exe = std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().to_ascii_lowercase())
        })
        .unwrap_or_default();
    if process_name.eq_ignore_ascii_case(&current_exe) {
        return Err(AppError::validation("Lumen 自身窗口不能作为场景采集目标"));
    }
    let window_title = window
        .title()
        .map_err(|_| AppError::validation("无法读取前台窗口标题；本次未采集画面"))?;
    Ok(ForegroundWindowInfo {
        process_name: process_name.to_ascii_lowercase(),
        window_title,
    })
}

/// Read only the foreground process first. The monitor calls this before it has
/// established that any enabled rule allows the application, so it does not read
/// arbitrary window titles or capture frames from unrelated apps.
#[cfg(windows)]
pub fn foreground_process_name() -> AppResult<String> {
    let window = Window::foreground().map_err(|_| AppError::validation("没有可采集的前台窗口"))?;
    if !window.is_valid() {
        return Err(AppError::validation("当前前台窗口不能作为采集目标"));
    }
    let process_name = window
        .process_name()
        .map_err(|_| AppError::validation("无法确认前台窗口所属程序"))?;
    let current_exe = std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().to_ascii_lowercase())
        })
        .unwrap_or_default();
    if process_name.eq_ignore_ascii_case(&current_exe) {
        return Err(AppError::validation("Lumen 自身窗口不能作为场景采集目标"));
    }
    Ok(process_name.to_ascii_lowercase())
}

#[cfg(windows)]
pub fn foreground_info_if_process(expected_process: &str) -> AppResult<ForegroundWindowInfo> {
    let window = Window::foreground().map_err(|_| AppError::validation("没有可采集的前台窗口"))?;
    let process_name = window
        .process_name()
        .map_err(|_| AppError::validation("无法确认前台窗口所属程序"))?;
    if !process_name.eq_ignore_ascii_case(expected_process) {
        return Err(AppError::validation("读取窗口标题前，前台程序已发生变化"));
    }
    foreground_info_for(&window)
}

#[cfg(not(windows))]
pub fn foreground_process_name() -> AppResult<String> {
    Err(AppError::validation("智能场景便签只支持 Windows。"))
}

#[cfg(not(windows))]
pub fn foreground_info_if_process(_expected_process: &str) -> AppResult<ForegroundWindowInfo> {
    Err(AppError::validation("智能场景便签只支持 Windows。"))
}

#[cfg(not(windows))]
pub fn foreground_info() -> AppResult<ForegroundWindowInfo> {
    Err(AppError::validation("智能场景便签只支持 Windows。"))
}

#[cfg(windows)]
type CapturedFrame = (Vec<u8>, u32, u32);

#[cfg(windows)]
type CaptureResult = Result<CapturedFrame, String>;

#[cfg(windows)]
struct OneShotCapture {
    result: mpsc::SyncSender<CaptureResult>,
}

#[cfg(windows)]
impl GraphicsCaptureApiHandler for OneShotCapture {
    type Flags = mpsc::SyncSender<CaptureResult>;
    type Error = String;

    fn new(context: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self {
            result: context.flags,
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        let result = (|| {
            let buffer = frame
                .buffer()
                .map_err(|error| format!("frame_buffer:{error}"))?;
            let width = buffer.width();
            let height = buffer.height();
            if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 64_000_000 {
                return Err("frame_size_unsupported".to_owned());
            }
            let mut scratch = Vec::new();
            let pixels = buffer.as_nopadding_buffer(&mut scratch).to_vec();
            let image = image::RgbaImage::from_raw(width, height, pixels)
                .ok_or_else(|| "frame_layout_invalid".to_owned())?;
            let mut png = Cursor::new(Vec::new());
            image
                .write_to(&mut png, image::ImageFormat::Png)
                .map_err(|error| format!("frame_encode:{error}"))?;
            Ok((png.into_inner(), width, height))
        })();
        let _ = self.result.send(result.clone());
        control.stop();
        result.map(|_| ())
    }
}

/// Capture the exact foreground HWND. It is rechecked after capture so a frame from a window
/// that lost focus during setup is discarded.
#[cfg(windows)]
pub fn capture_foreground() -> AppResult<ForegroundFrame> {
    if !platform_supported() {
        return Err(AppError::validation(
            "Windows Graphics Capture 需要 Windows 10 1903（18362）或更高版本；没有改用全屏截图。",
        ));
    }
    let window = Window::foreground().map_err(|_| AppError::validation("没有可采集的前台窗口"))?;
    if !window.is_valid() {
        return Err(AppError::validation("当前前台窗口不能作为采集目标"));
    }
    let info = foreground_info_for(&window)?;
    if Window::foreground().is_ok_and(|current| current != window) {
        return Err(AppError::validation("前台窗口已变化；本次未采集画面"));
    }
    let (sender, receiver) = mpsc::sync_channel(1);
    let settings = Settings::new(
        window,
        CursorCaptureSettings::WithoutCursor,
        DrawBorderSettings::WithoutBorder,
        SecondaryWindowSettings::Exclude,
        MinimumUpdateIntervalSettings::Default,
        DirtyRegionSettings::Default,
        ColorFormat::Rgba8,
        sender,
    );
    let control = OneShotCapture::start_free_threaded(settings)
        .map_err(|_| AppError::internal("窗口采集启动失败（SCENE_CAPTURE_START）"))?;
    let (png, width, height) = match receiver.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(frame)) => frame,
        Ok(Err(_)) => {
            let _ = control.stop();
            return Err(AppError::internal("窗口帧处理失败（SCENE_CAPTURE_FRAME）"));
        }
        Err(_) => {
            let _ = control.stop();
            return Err(AppError::internal("窗口采集超时（SCENE_CAPTURE_TIMEOUT）"));
        }
    };
    let _ = control.stop();
    let current = Window::foreground()
        .map_err(|_| AppError::validation("采集期间前台窗口发生变化；已丢弃画面"))?;
    if current != window {
        return Err(AppError::validation("采集期间前台窗口发生变化；已丢弃画面"));
    }
    Ok(ForegroundFrame {
        process_name: info.process_name,
        window_title: info.window_title,
        width,
        height,
        png,
    })
}

#[cfg(not(windows))]
pub fn capture_foreground() -> AppResult<ForegroundFrame> {
    Err(AppError::validation(
        "智能场景便签的前台窗口采集仅在受支持的 Windows 版本运行。",
    ))
}

#[cfg(windows)]
pub fn platform_supported() -> bool {
    #[repr(C)]
    struct VersionInfo {
        size: u32,
        major: u32,
        minor: u32,
        build: u32,
        platform: u32,
        service_pack: [u16; 128],
    }

    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn RtlGetVersion(info: *mut VersionInfo) -> i32;
    }

    let mut info: VersionInfo = unsafe { std::mem::zeroed() };
    info.size = std::mem::size_of::<VersionInfo>() as u32;
    unsafe {
        RtlGetVersion(&mut info) == 0
            && (info.major > 10 || (info.major == 10 && info.build >= 18_362))
    }
}

#[cfg(not(windows))]
pub fn platform_supported() -> bool {
    false
}
