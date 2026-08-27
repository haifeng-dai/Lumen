// Windows GUI 应用配置：不显示控制台窗口
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use database::state::LocalStateManager;
use gpui::{
    App, AppContext, AsyncApp, Bounds, KeyBinding, Point, WindowBounds, WindowOptions, px, size,
};
use gpui_component::{Root, TitleBar};
use i18n::Language;
use log::{debug, error, info};
use lumen::{
    RUNTIME,
    app_state::config::ConfigStore,
    app_state::data::DataStore,
    app_state::theme::{SurfaceState, ThemeLoaderState},
    assets::Assets,
    ui::actions::{
        CloseWindow, Copy, Cut, HideApp, HideOtherApps, MinimizeWindow, Paste, Quit, Redo,
        SelectAll, ShowAllApps, ToggleFullscreen, Undo, ZoomWindow,
    },
    ui::views::main_window::{MainWindow, ShowSettings, build_app_menus},
};
use models::config::AppConfig;
use services::{
    app::MainApp,
    config::get_app_root_dir,
    file_monitor::{FileEvent, FileMonitorService},
    query::data::{AppViewMode, SortField, SortOrder},
};
use std::sync::{Arc, LazyLock, atomic::Ordering};

mod bootstrap;
mod logging;

#[cfg(target_os = "macos")]
use bootstrap::set_mac_app_name;
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
use bootstrap::{notify_running_instance, start_socket_listener};
use logging::{init_logger_with_path, setup_panic_hook, setup_stderr_redirection};

fn main() {
    // 0. 设置崩溃捕获
    setup_panic_hook();

    // 0.0 macOS：未打包运行（cargo run）时，菜单栏左侧应用菜单标题默认取进程名
    // "lumen"（二进制名）。将其设为 "Lumen"，使开发期也与打包后（Info.plist 的
    // CFBundleName）一致。必须在 AppKit 建立主菜单前调用。
    #[cfg(target_os = "macos")]
    set_mac_app_name();

    // 0.1 Linux 单实例检查：如果已有实例在运行，通知其激活窗口后退出
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    if notify_running_instance() {
        return;
    }

    // 1. 初始化本地状态管理器 (state.db) — 提前到配置加载之前
    let config_dir = get_app_root_dir();
    let local_state_manager = Arc::new(LocalStateManager::new(config_dir));

    if let Err(e) = local_state_manager.init() {
        eprintln!("[FATAL] 本地状态数据库初始化失败: {e:?}");
        error!("无法初始化本地状态数据库: {e}");
    }

    // 2. 从 state.db 加载配置，不存在则创建默认配置并写入
    //    加载/解析/默认值回落逻辑委托给服务层 services::config::load_config
    let config: AppConfig = services::config::load_config(&local_state_manager);

    // 应用代理环境变量
    services::config::apply_proxy_config(&config.proxy);

    // 确保数据目录存在
    if let Err(e) = services::config::ensure_dirs(&config) {
        eprintln!("无法创建应用目录: {e}");
    }

    // 4. 加载初始 UI 状态
    let initial_state = local_state_manager.load_all().unwrap_or_else(|e| {
        error!("加载本地状态失败: {e}, 将使用默认状态");
        Default::default()
    });

    // 5. 记录主题目录，稍后在 GPUI App 上下文中加载主题缓存（Global）
    let themes_dir = config.themes_dir();

    // 7. 初始化日志 (依赖配置)
    services::config::clean_old_logs(&config);

    let log_path = config.get_current_log_path();
    init_logger_with_path(&config, &log_path);

    // 8. 重定向 stderr
    setup_stderr_redirection(&log_path);

    // Ensure runtime is initialized
    LazyLock::force(&RUNTIME);
    info!("开始初始化应用...");

    gpui_platform::application().with_assets(Assets).run({
        let local_state_manager = local_state_manager.clone();
        move |cx: &mut App| {
            // 1. 初始化 UI 组件库环境
            gpui_component::init(cx);

            // 1.1 注册 ConfigStore Global（配置访问的统一入口）
            ConfigStore::load_and_set(&local_state_manager, cx);

            // 1.1.0 注册 SurfaceState / ThemeLoaderState Global（主题运行时态）
            SurfaceState::init(cx);
            {
                let mut loader = services::theme::ThemeLoader::new();
                let _ = loader.load_all(&themes_dir);
                cx.set_global(ThemeLoaderState { loader });
            }

            // 1.1.1 注册配置变更观察者 (观察者模式迁移)
            cx.observe_global::<ConfigStore>(|cx| {
                let config = cx.global::<ConfigStore>().inner.clone();
                let scale_val = config.ui.ui_scale;

                // 应用代理环境变量
                services::config::apply_proxy_config(&config.proxy);

                // 应用主题到全局 Theme
                lumen::ui::apply_theme(
                    &config.ui.theme_mode,
                    &config.ui.theme_style,
                    scale_val,
                    cx,
                );

                // 同步更新所有窗口的 rem_size（各窗口自行通过 observe_global 触发 cx.notify）
                for window_handle in cx.windows() {
                    let _ = cx.update_window(window_handle, move |_, window, _| {
                        window.set_rem_size(px(16.0 * scale_val));
                    });
                }
            })
            .detach();

            // 1.2 注册嵌入的 KaTeX 原生数学字体 (位于 assets/fonts/)
            let font_sources = lumen::assets::Assets::fonts();
            if let Err(e) = cx.text_system().add_fonts(font_sources) {
                log::warn!("KaTeX 数学字体注册失败: {e}");
            } else {
                info!("KaTeX 原生数学字体已成功从 assets/fonts 注册到全局 TextSystem");
            }

            // 1.3 初始化 PDF 阅读器模块 (可选)
            info!("PDF Viewer 模块已加载");

            // 1.5 设置菜单（MacOS 全屏呼出菜单栏依赖菜单配置）
            // 菜单结构按视图模式构建：文献库 / 订阅 菜单会随视图切换，
            // 其余 App / Edit / View / Window 为标准 macOS 原生菜单。
            let lang = config.ui.language.parse::<Language>().unwrap_or_default();

            cx.set_menus(build_app_menus(AppViewMode::Library, lang));

            // 2. 初始化应用全局控制器
            let (app_controller_struct, sync_rx) = MainApp::new(
                config.clone(),
                local_state_manager.clone(),
                initial_state.clone(),
            );

            // 2.1.1 初始化 DataStore GPUI Entity（领域数据的新权威源）
            let data_store: lumen::app_state::data::DataStoreEntity =
                cx.new(|_cx| DataStore::new(app_controller_struct.db.clone()));
            data_store.update(cx, |store, cx| {
                if let Err(e) = store.refresh_from_db(cx) {
                    error!("DataStore: refresh_from_db 失败: {e}");
                }
            });

            // 2.2 初始化 NotificationBus Global（通知系统总线）
            cx.set_global(lumen::ui::notification::NotificationBus::new());

            // 2.3 初始化 UiState Global 并应用持久化的初始状态
            cx.set_global(lumen::app_state::ui::UiState::new());
            {
                let state = cx.global_mut::<lumen::app_state::ui::UiState>();
                if let Some(sidebar_item) = &initial_state.selected_sidebar_item {
                    if let Some(folder_id) = sidebar_item.strip_prefix("folder:") {
                        state.selected_folder_id = Some(folder_id.to_string());
                    } else if let Some(tag_id) = sidebar_item.strip_prefix("tag:") {
                        state.selected_tag_id = Some(tag_id.to_string());
                    }
                }
                if let Some(field) = &initial_state.sort_field {
                    state.sort_field = match field.as_str() {
                        "Title" => SortField::Title,
                        "Author" => SortField::Author,
                        "Year" => SortField::Year,
                        "Journal" => SortField::Journal,
                        _ => SortField::Title,
                    };
                }
                state.sort_order = if initial_state.sort_asc {
                    SortOrder::Ascending
                } else {
                    SortOrder::Descending
                };
            }

            let app_controller = Arc::new(app_controller_struct);

            // 启动自动同步后台循环
            app_controller
                .sync_service
                .clone()
                .start_auto_sync_loop(sync_rx);

            // 启动订阅后台更新循环
            let notify = Arc::new({
                let app = app_controller.clone();
                move || app.notify_data_changed()
            });
            app_controller
                .feed_service
                .clone()
                .start_background_loop(app_controller.db.clone(), notify);

            // 3. 绑定退出快捷键
            let mut key_bindings = vec![
                KeyBinding::new(
                    if cfg!(target_os = "macos") {
                        "cmd-q"
                    } else {
                        "ctrl-q"
                    },
                    Quit,
                    None,
                ),
                KeyBinding::new(
                    if cfg!(target_os = "macos") {
                        "cmd-w"
                    } else {
                        "ctrl-w"
                    },
                    CloseWindow,
                    None,
                ),
            ];

            // 仅在 macOS 上启用全屏快捷键
            if cfg!(target_os = "macos") {
                key_bindings.push(KeyBinding::new("ctrl-cmd-f", ToggleFullscreen, None));
                info!("注册 macOS 全屏快捷键: ctrl-cmd-f");

                // 标准 macOS 菜单快捷键
                key_bindings.push(KeyBinding::new("cmd-,", ShowSettings, None)); // 设置
                key_bindings.push(KeyBinding::new("cmd-h", HideApp, None)); // 隐藏 Lumen
                key_bindings.push(KeyBinding::new("alt-cmd-h", HideOtherApps, None)); // 隐藏其他
                key_bindings.push(KeyBinding::new("cmd-m", MinimizeWindow, None)); // 最小化窗口
            } else {
                info!("Windows/Linux 暂不启用全屏快捷键 (框架兼容性限制)");
            }

            cx.bind_keys(key_bindings);

            // 4. 注册退出处理器 (在此处保存状态)
            let lsm_for_quit = local_state_manager.clone();
            let app_for_quit = app_controller.clone();

            cx.on_action(move |_: &Quit, cx| {
                if let Ok(state) = app_for_quit.local_state.read()
                    && let Err(e) = lsm_for_quit.save_all(&state)
                {
                    error!("保存本地状态失败: {e}");
                }
                #[cfg(any(target_os = "linux", target_os = "freebsd"))]
                {
                    let sock_path = get_app_root_dir().join("lumen.sock");
                    let _ = std::fs::remove_file(&sock_path);
                }
                cx.quit();
            });

            // 注册 CloseWindow 处理器
            cx.on_action(move |_: &CloseWindow, cx| {
                info!("触发 CloseWindow Action (Cmd+W)");
                if let Some(window) = cx.active_window() {
                    cx.update_window(window, |_, window, _| {
                        window.remove_window();
                    })
                    .ok();
                }
            });

            // 注册 ToggleFullscreen 处理器
            cx.on_action(move |_: &ToggleFullscreen, cx| {
                info!("触发 ToggleFullscreen Action");
                if let Some(window) = cx.active_window() {
                    cx.update_window(window, |_, window, _| {
                        window.toggle_fullscreen();
                    })
                    .ok();
                }
            });

            // 标准 macOS 菜单：隐藏/显示应用
            cx.on_action(move |_: &HideApp, cx| {
                cx.hide();
            });
            cx.on_action(move |_: &HideOtherApps, cx| {
                cx.hide_other_apps();
            });
            cx.on_action(move |_: &ShowAllApps, cx| {
                cx.unhide_other_apps();
            });

            // 标准 macOS 菜单：窗口管理（作用于当前活动窗口）
            cx.on_action(move |_: &MinimizeWindow, cx| {
                if let Some(window) = cx.active_window() {
                    cx.update_window(window, |_, window, _| {
                        window.minimize_window();
                    })
                    .ok();
                }
            });
            cx.on_action(move |_: &ZoomWindow, cx| {
                if let Some(window) = cx.active_window() {
                    cx.update_window(window, |_, window, _| {
                        window.zoom_window();
                    })
                    .ok();
                }
            });

            // 编辑菜单 action：转发到当前活动窗口的焦点元素。
            // 注意：gpui_component 的输入框目前只监听其私有 action，
            // 因此这些菜单项默认“可用”(已注册 handler) 但点击是否真正执行
            // 剪贴/撤销，取决于焦点元素是否处理同名 action。键盘快捷键
            // (⌘C/⌘Z 等) 已由 gpui_component 自行处理，不受影响。
            macro_rules! forward_edit_action {
                ($action:ident) => {
                    cx.on_action(move |_: &$action, cx| {
                        if let Some(window) = cx.active_window() {
                            cx.update_window(window, |_, window, cx| {
                                window.dispatch_action(Box::new($action), cx);
                            })
                            .ok();
                        }
                    });
                };
            }
            forward_edit_action!(Undo);
            forward_edit_action!(Redo);
            forward_edit_action!(Cut);
            forward_edit_action!(Copy);
            forward_edit_action!(Paste);
            forward_edit_action!(SelectAll);

            // 5. 打开主窗口并使用 Root 包裹
            // 获取主显示器信息，计算自适应窗口大小
            let display = cx.primary_display();
            let screen_size = display
                .as_ref()
                .map_or_else(|| size(px(1920.0), px(1080.0)), |d| d.bounds().size);

            // 最小窗口尺寸
            let min_width = 900.0_f32;
            let min_height = 600.0_f32;

            // 根据屏幕尺寸计算默认窗口大小
            // 策略：窗口占屏幕可用区域的 75%，但有最小 and 最大限制
            let screen_width = f32::from(screen_size.width);
            let screen_height = f32::from(screen_size.height);

            let default_width = (screen_width * 0.75).clamp(min_width, 1600.0);
            let default_height = (screen_height * 0.75).clamp(min_height, 1000.0);

            // 从保存的状态恢复窗口尺寸，或使用计算的默认值
            let window_state = &initial_state.window_state;

            let width = window_state
                .width
                .map_or(default_width, |w| (w as f32).clamp(min_width, screen_width));
            let height = window_state.height.map_or(default_height, |h| {
                (h as f32).clamp(min_height, screen_height)
            });

            // 验证保存的位置是否仍在屏幕范围内
            let bounds = if let (Some(x), Some(y)) = (window_state.x, window_state.y) {
                let x = x as f32;
                let y = y as f32;
                // 确保窗口至少有 100px 在屏幕内可见
                let visible_margin = 100.0;
                if x > -width + visible_margin
                    && x < screen_width - visible_margin
                    && y > -height + visible_margin
                    && y < screen_height - visible_margin
                {
                    // 恢复保存的位置和尺寸
                    Bounds {
                        origin: Point::new(px(x), px(y)),
                        size: size(px(width), px(height)),
                    }
                } else {
                    // 位置无效，居中显示
                    Bounds::centered(None, size(px(width), px(height)), cx)
                }
            } else {
                // 居中显示
                Bounds::centered(None, size(px(width), px(height)), cx)
            };

            let is_maximized = window_state.is_maximized || window_state.width.is_none();

            cx.open_window(
                WindowOptions {
                    window_bounds: Some(if window_state.is_fullscreen {
                        WindowBounds::Fullscreen(bounds)
                    } else if is_maximized {
                        WindowBounds::Maximized(bounds)
                    } else {
                        WindowBounds::Windowed(bounds)
                    }),
                    window_min_size: Some(size(px(min_width), px(min_height))),
                    titlebar: Some(TitleBar::title_bar_options()),
                    app_owns_titlebar_drag: true,
                    app_id: Some("Lumen".to_string()),
                    ..Default::default()
                },
                {
                    let app_controller = app_controller.clone();
                    let lsm_for_close = local_state_manager.clone();
                    let _ui_scale = config.ui.ui_scale;

                    let data_store_for_window = data_store.clone();
                    move |window, cx| {
                        // 全局唯一退出标记，防止双重 cx.quit()
                        let quit_triggered = Arc::new(std::sync::atomic::AtomicBool::new(false));

                        let main_window = cx.new(|cx| {
                            MainWindow::new(
                                app_controller.clone(),
                                data_store_for_window.clone(),
                                window,
                                cx,
                            )
                        });

                        // 监听窗口尺寸变化，实时更新本地状态（内存），对齐 Zed 官方提取 window.window_bounds() 的逻辑
                        let app_ctrl_bounds = app_controller.clone();
                        let bounds_subscription = main_window.update(cx, |_, cx| {
                            cx.observe_window_bounds(window, move |_, window, _| {
                                let window_bounds = window.window_bounds();
                                if let Ok(mut state) = app_ctrl_bounds.local_state.write() {
                                    match window_bounds {
                                        WindowBounds::Windowed(bounds) => {
                                            if f32::from(bounds.size.width) >= 200.0 && f32::from(bounds.size.height) >= 200.0 {
                                                state.window_state.width = Some(f64::from(bounds.size.width));
                                                state.window_state.height = Some(f64::from(bounds.size.height));
                                                state.window_state.x = Some(f64::from(bounds.origin.x));
                                                state.window_state.y = Some(f64::from(bounds.origin.y));
                                                state.window_state.is_maximized = false;
                                                state.window_state.is_fullscreen = false;
                                            }
                                        }
                                        WindowBounds::Maximized(bounds) => {
                                            // 处于最大化时，保存恢复正常尺寸时的基础 bounds（Windows/Linux 专属逻辑）
                                            if f32::from(bounds.size.width) >= 200.0 && f32::from(bounds.size.height) >= 200.0 {
                                                state.window_state.width = Some(f64::from(bounds.size.width));
                                                state.window_state.height = Some(f64::from(bounds.size.height));
                                                state.window_state.x = Some(f64::from(bounds.origin.x));
                                                state.window_state.y = Some(f64::from(bounds.origin.y));
                                                state.window_state.is_maximized = true;
                                                state.window_state.is_fullscreen = false;
                                            }
                                        }
                                        WindowBounds::Fullscreen(bounds) => {
                                            if f32::from(bounds.size.width) >= 200.0 && f32::from(bounds.size.height) >= 200.0 {
                                                state.window_state.width = Some(f64::from(bounds.size.width));
                                                state.window_state.height = Some(f64::from(bounds.size.height));
                                                state.window_state.x = Some(f64::from(bounds.origin.x));
                                                state.window_state.y = Some(f64::from(bounds.origin.y));
                                                state.window_state.is_maximized = false;
                                                state.window_state.is_fullscreen = true;
                                            }
                                        }
                                    }
                                }
                            })
                        });

                        // 增加主窗口实体释放监听，作为窗口关闭退出的可靠补充
                        // 这解决了 macOS 下窗口关闭时 cx.windows() 可能仍包含已关闭窗口的问题
                        let lsm_for_observe = local_state_manager.clone();
                        let app_ctrl_for_observe = app_controller.clone();
                        let quit_flag_for_observe = quit_triggered.clone();
                        cx.observe_release(&main_window, move |_, cx| {
                            info!("主窗口实体已释放 (MainWindow observe_release)");
                            // 使用 compare_exchange 确保 quit() 只调用一次
                            if quit_flag_for_observe
                                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                                .is_ok()
                            {
                                if let Ok(state) = app_ctrl_for_observe.local_state.read()
                                    && let Err(e) = lsm_for_observe.save_all(&state)
                                {
                                    error!("保存本地状态失败: {e}");
                                }
                                cx.quit();
                            }
                        })
                        .detach();

                        // 监听窗口关闭事件，保存状态到数据库并退出应用
                        let lsm = lsm_for_close.clone();
                        let app_ctrl = app_controller.clone();
                        let main_window_handle = window.window_handle();
                        let quit_flag_for_close = quit_triggered.clone();

                        let close_subscription = cx.on_window_closed(move |cx, _window| {
                            debug!("WINDOW_CLOSE: 窗口关闭事件触发");

                            // 使用 cx.windows() 而非 cx.window_stack()，因为 stack 在某些平台（如 Windows）
                            // 可能在窗口关闭时的行为不符合预期（例如返回空）。
                            // cx.windows() 返回当前应用所有活跃窗口的句柄列表。
                            let windows = cx.windows();
                            let window_count = windows.len();

                            // 检查主窗口句柄是否在当前的窗口列表中
                            let main_any_handle: gpui::AnyWindowHandle = main_window_handle;
                            let has_main = windows.contains(&main_any_handle);

                            debug!("WINDOW_CLOSE: 当前窗口数={window_count}, 主窗口是否在列表={has_main}");

                            // 统一跨平台逻辑：
                            // 1. 如果主窗口不在了 (!has_main)，说明主窗口已关闭 -> 退出。
                            // 2. 如果窗口总数为 0，说明所有窗口都关了 -> 退出。
                            let should_quit = window_count == 0 || !has_main;

                            if should_quit {
                                // 使用 compare_exchange 确保 quit() 只调用一次
                                if quit_flag_for_close
                                    .compare_exchange(
                                        false,
                                        true,
                                        Ordering::SeqCst,
                                        Ordering::SeqCst,
                                    )
                                    .is_ok()
                                {
                                    if let Ok(state) = app_ctrl.local_state.read()
                                        && let Err(e) = lsm.save_all(&state)
                                    {
                                        error!("保存本地状态失败: {e}");
                                    }
                                    info!(
                                        "满足退出条件 (count={} or main_lost={})，执行 cx.quit()",
                                        window_count, !has_main
                                    );
                                    cx.quit();
                                }
                            }
                        });

                        // 将订阅存储到 MainWindow 中以保持其生命周期
                        main_window.update(cx, |mw, _| {
                            mw.bounds_subscription = Some(bounds_subscription);
                            mw.close_subscription = Some(close_subscription);
                        });

                        cx.new(|cx| Root::new(main_window.clone(), window, cx))
                    }
                },
            )
            .expect("无法打开窗口");

            // 窗口创建后初始化统一文件监控服务
            let attachments_dir = app_controller
                .sync_service
                .file_manager
                .get_attachments_dir();
            let app_for_monitor = app_controller.clone();
            if let Some((watcher, mut rx)) = FileMonitorService::new(
                attachments_dir,
                config.themes_dir(),
            ) {
                cx.spawn(|wcx: &mut AsyncApp| {
                    let wcx = wcx.clone();
                    async move {
                        let _keep = watcher;
                        while let Some(event) = rx.recv().await {
                            match event {
                                FileEvent::AttachmentChanged(path) => {
                                    let path_str = path.to_string_lossy().to_string();
                                    let db = &app_for_monitor.db;

                                    let mut found_att = db
                                        .get_attachment_by_file_path(&path_str)
                                        .ok()
                                        .flatten();

                                    if found_att.is_none()
                                        && let Some(file_name) =
                                            path.file_name().and_then(|n| n.to_str())
                                        && let Ok(candidates) =
                                            db.get_attachments_by_file_name(file_name)
                                    {
                                        let changed_canonical =
                                            std::fs::canonicalize(&path).ok();
                                        for att in candidates {
                                            let db_canonical = std::fs::canonicalize(
                                                std::path::Path::new(&att.file_path),
                                            )
                                            .ok();
                                            if let (Some(p1), Some(p2)) =
                                                (&changed_canonical, &db_canonical)
                                                && p1 == p2
                                            {
                                                found_att = Some(att);
                                                break;
                                            }
                                        }
                                    }

                                    if let Some(att) = found_att {
                                        info!(
                                            "文件监控: 附件 [{}] 检测到变更，标记为需要同步",
                                            att.id
                                        );
                                        if let Err(e) = db.mark_attachment_dirty(&att.id) {
                                            error!("文件监控: 标记附件失败: {e}");
                                        } else {
                                            app_for_monitor.notify_data_changed();
                                        }
                                    } else {
                                        debug!("文件监控: 未找到对应附件记录: {path_str}");
                                    }
                                }
                                FileEvent::ThemeChanged(paths) => {
                                    let (mode, style, scale) = {
                                        let config = app_for_monitor.config.lock().unwrap();
                                        (
                                            config.ui.theme_mode.clone(),
                                            config.ui.theme_style.clone(),
                                            config.ui.ui_scale,
                                        )
                                    };
                                    wcx.update(|cx: &mut App| {
                                        {
                                            let mut loader =
                                                cx.global::<ThemeLoaderState>().loader.clone();
                                            for p in &paths {
                                                if let Err(e) = loader.reload_theme_from_file(p) {
                                                    error!("文件监控: 重载主题失败: {e}");
                                                }
                                            }
                                            cx.set_global(ThemeLoaderState { loader });
                                        }
                                        lumen::ui::apply_theme(&mode, &style, scale, cx);
                                    });
                                }
                            }
                        }
                    }
                })
                .detach();
            }

            // 在 Linux 上启动单实例 socket 监听
            #[cfg(any(target_os = "linux", target_os = "freebsd"))]
            {
                let (tx, rx) = std::sync::mpsc::channel();
                start_socket_listener(tx);
                let recv = rx;
                cx.spawn(move |cx: &mut AsyncApp| {
                    let cx = cx.clone();
                    async move {
                        loop {
                            while recv.try_recv().is_ok() {
                                info!("收到激活信号，聚焦窗口");
                                cx.update(|cx| cx.activate(true));
                            }
                            cx.background_executor()
                                .timer(std::time::Duration::from_millis(200))
                                .await;
                        }
                    }
                })
                .detach();
            }

            cx.activate(true);
            info!("应用启动完成");
        }
    })
}
