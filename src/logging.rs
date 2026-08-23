use env_logger::{Builder, Target};
use log::{LevelFilter, error, info, logger};
use models::config::AppConfig;
#[cfg(unix)]
use std::os::unix::io::AsRawFd;
use std::{
    fs::{OpenOptions, create_dir_all},
    io::Write,
    panic::set_hook,
    path::Path,
};

pub(crate) fn setup_stderr_redirection(log_path: &std::path::Path) {
    if let Ok(file) = OpenOptions::new().create(true).append(true).open(log_path) {
        let fd = file.as_raw_fd();
        unsafe {
            libc::dup2(fd, libc::STDERR_FILENO);
        }
        info!("系统 stderr 已合并重定向至应用日志文件");
    } else {
        error!("无法重定向系统 stderr");
    }
}

#[cfg(windows)]
pub(crate) fn setup_stderr_redirection(log_path: &std::path::Path) {
    if let Ok(file) = OpenOptions::new().create(true).append(true).open(log_path) {
        unsafe {
            let handle = file.into_raw_handle();
            // 0 is typically text mode, usually safe for logs. O_APPEND equivalent might be needed if not implicit?
            // Windows CRT append mode behavior with fd open might vary, but we opened the file with append option.
            let fd = libc::open_osfhandle(handle as isize, 0);
            if fd != -1 {
                libc::dup2(fd, 2); // 2 is stderr
            }
        }
        info!("系统 stderr 已合并重定向至应用日志文件");
    } else {
        error!("无法重定向系统 stderr");
    }
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn setup_stderr_redirection(_: &std::path::Path) {}

pub(crate) fn setup_panic_hook() {
    set_hook(Box::new(|panic_info| {
        let location = panic_info.location().map_or_else(
            || "unknown".to_string(),
            |l| format!("{}:{}", l.file(), l.line()),
        );
        let payload = panic_info.payload();
        let message = if let Some(s) = payload.downcast_ref::<&str>() {
            *s
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s.as_str()
        } else {
            "Box<Any>"
        };

        let backtrace = std::backtrace::Backtrace::capture();
        error!("CRITICAL: 应用发生崩溃 (Panic)!");
        error!("位置: {location}");
        error!("信息: {message}");
        error!("堆栈跟踪:\n{backtrace}");

        // 确保日志被刷入磁盘
        logger().flush();
    }));
}

pub(crate) fn init_logger_with_path(config: &AppConfig, log_path: &Path) {
    // 确保日志目录存在
    if let Some(parent) = log_path.parent() {
        let _ = create_dir_all(parent);
    }

    // 使用追加模式打开日志文件，防止启动时抹除崩溃日志
    let target_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .expect("无法打开日志文件");

    let target = Box::new(target_file);
    Builder::new()
        .format(|buf, record| {
            writeln!(
                buf,
                "{} [{}] - {}",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
                record.level(),
                record.args()
            )
        })
        .filter_level(LevelFilter::Trace) // 设到最宽松，运行时用 set_max_level_filter 控制
        .target(Target::Pipe(target))
        .init();
    // 将实际日志级别控制在配置值
    log::set_max_level(
        config
            .log_level
            .parse::<LevelFilter>()
            .unwrap_or(LevelFilter::Info),
    );

    info!("----------------------------------------------------------------");
    info!("日志系统已启动 (追加模式)，日志文件: {log_path:?}");
}
