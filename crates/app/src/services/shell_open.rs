//! 系统 shell 打开器（无 UI 自由函数）：系统浏览器打开 html/htm、资源
//! 管理器打开目录、文件定位选中（explorer /select, / macOS open -R）。
//! Windows + macOS 双实现。
//! （P2 归并：actions_panels / actions_files 的 OS 打开器。）

use std::path::Path;

/// 删除到系统回收站 / 废纸篓（021 文件树删除、052 会话清理、扩展卸载共用）。
///
/// **macOS 必须显式选 NSFileManager 通道**：`trash` crate 在 macOS 的默认实现
/// 是 `osascript` 呼叫 Finder，首次使用会弹「pi-flash 想要控制访达」的自动化
/// 授权框（拒绝 = 删除直接失败），还会响一声 Finder 的删除音；而
/// `NSFileManager.trashItemAtURL` 不要任何额外授权、更快、无声音——删掉的文件
/// 一样进废纸篓。Windows 保持默认通道（回收站 + 已配的 COM 初始化）。
pub(crate) fn trash_delete(path: &Path) -> Result<(), trash::Error> {
    #[cfg(target_os = "macos")]
    {
        use trash::macos::{DeleteMethod, TrashContextExtMacos};
        let mut ctx = trash::TrashContext::new();
        ctx.set_delete_method(DeleteMethod::NsFileManager);
        return ctx.delete(path);
    }
    #[cfg(not(target_os = "macos"))]
    {
        trash::delete(path)
    }
}

/// 用系统默认浏览器打开文件（html/htm）。
pub(crate) fn open_in_browser(path: &std::path::Path) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", ""])
            .arg(path)
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(path).spawn();
    }
}

/// 用系统默认浏览器打开 URL（040-2 市场页的 npm/repo/report 链接）。
pub(crate) fn open_url(url: &str) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
}

/// 用系统默认应用打开文件（editor「无法打开」灰屏的兜底动作）：cmd start /
/// macOS open 本就按文件关联选应用，与 open_in_browser 同实现异名——
/// 调用点语义是「默认应用」而非「浏览器」。
pub(crate) fn open_in_default_app(path: &Path) {
    open_in_browser(path);
}

/// 在系统文件浏览器中打开目录（Windows explorer / macOS open）。
pub(crate) fn open_in_explorer(path: &std::path::Path) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer").arg(path).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(path).spawn();
    }
}

/// 「在资源管理器中打开」的文件语义（与 open_in_explorer 的目录语义分
/// 开）：文件 = 打开所在目录并选中（explorer /select, / macOS open -R），
/// 目录 = 直接进入。
pub(crate) fn reveal_in_explorer(path: &Path) {
    #[cfg(windows)]
    {
        if path.is_dir() {
            let _ = std::process::Command::new("explorer").arg(path).spawn();
        } else {
            let _ = std::process::Command::new("explorer")
                .arg(format!("/select,{}", path.display()))
                .spawn();
        }
    }
    #[cfg(target_os = "macos")]
    {
        if path.is_dir() {
            let _ = std::process::Command::new("open").arg(path).spawn();
        } else {
            let _ = std::process::Command::new("open")
                .arg("-R")
                .arg(path)
                .spawn();
        }
    }
}
