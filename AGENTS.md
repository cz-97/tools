# AGENTS.md

## 1. 项目目标

一个基于 Rust 和 GPUI 的 Windows 原生启动器，个人自用。

核心交互：

1. 通过 Ctrl+Space 唤起启动器。
2. 输入关键词过滤命令。
3. 上下方向键选择。
4. Enter 执行选中项。
5. Esc 隐藏窗口。
6. 拖动搜索栏空白处移动面板（纵坐标会被记住）。
7. 搜索栏右侧的齿轮图标打开设置页。

不追求成为通用启动器框架，也不做插件扩展体系。命令是编译进主程序的静态表。

## 2. 技术原则

- Rust + GPUI，不引入 Electron、WebView 或其他高开销 UI 框架。
- 优先使用轻量、职责明确的依赖。
- 避免为了抽象而抽象：不提前构建扩展体系、SDK、清单格式或进程协议。
- 阻塞操作不得放在 UI 线程。

## 3. 主程序职责

- 启动器窗口与搜索输入框。
- 全局快捷键与窗口生命周期。
- 窗口外壳：无边框、精确客户区尺寸、按显示器工作区居中（`src/window.rs`）。
- 命令列表的过滤、渲染与键盘导航。
- 命令的执行。
- 设置窗口（`SettingsView`）与设置持久化（`src/settings.rs`）。
- 系统托盘。

## 4. 命令

命令定义在 `src/main.rs` 的 `TOOLS` 表里。每条包含名称、描述、搜索关键词、图标和一个
`fn(&mut App)` 动作。

新增命令 = 往表里加一项 + 写一个动作函数。不要为此引入注册机制、清单文件或外部进程。

需要窗口的命令走 `with_launcher`。它把跳转延迟到当前 effect 周期之后，避免在按键分发
过程中重入窗口。

## 5. 列表 UI 约定

列表项布局：

- 左侧：图标。
- 右侧第一行：名称。
- 右侧第二行：描述。

行高、图标尺寸、选中态、hover 与截断规则由主程序统一实现（见 `ToolRow`），不要在各个
命令里各自处理。

## 5.1 搜索栏与拖动

搜索栏是面板唯一的拖动面，用 GPUI 的 `WindowControlArea::Drag` 交给系统处理（不要自己
拦鼠标事件模拟移动）：

- 搜索栏容器（`.id("search-bar")`）整体是拖动区。**需要鼠标的控件必须 `.occlude()`**，
  否则系统把按下的那一次判成「拖动标题栏」，控件永远收不到点击。
- 搜索框宽度跟随文字（`search_field_width`），不用 `flex_1`：它占的每一像素都是拖不动的
  区域。空查询时只留光标宽度，因此「没有文字时整条搜索栏（含占位提示）都能拖」。
- 占位提示是拖动区里的普通 `div`，不是 Input 的 placeholder —— 提示要能跟着被拖。
- 设置按钮在搜索栏右端，点击后先藏面板再开设置窗口（两个窗口不叠在一起）。

## 5.2 键盘

输入框是唯一持有焦点的控件，而 List 不再是焦点视图，所以**按键统一由
`App::intercept_keystrokes` 处理**（`enter` / `up` / `down` / `escape`），并通过
`with_launcher` 回到视图。拦截器要先用 `owns_window` 确认按键属于启动器窗口，否则设置
窗口里的按键会被启动器吞掉。

## 5.3 设置

设置存在 `%APPDATA%\gpui-tools\settings.ini`（`src/settings.rs`，`key = value` 文本，
坏行忽略、缺失取默认）：

- `theme` = `system` / `light` / `dark`。设置页改主题后立刻 `Theme::change` 并落盘。
- `window_y` = 面板顶边的物理像素。**只记纵坐标**，横坐标每次启动重新居中；恢复时按显示器
  工作区夹紧（`NativeWindow::center_x_at`）。写入时机：面板隐藏、退出命令、托盘退出。

## 6. 外部程序调用

调用外部程序一律使用参数数组（`Command::new(...).arg(...)`），不得把用户输入或任意文本
拼接成 Shell 命令。

## 7. 异步与性能

- 不得在 UI 线程执行阻塞式网络请求或大量磁盘操作。
- 过滤在输入时同步进行；命令数量很小，不需要防抖或后台任务。
- 不为简单功能引入后台服务或长期驻留进程。

## 8. 错误处理

- 外部调用失败只写入标准错误，不弹窗、不崩溃。
- 窗口句柄失效时返回 `false`，不 panic。

## 9. 本机 GPUI / gpui-component 的既有坑

这几条是实测踩出来的，改按键、拖动或跨实体逻辑前先读：

- **`Window::start_window_move` 在 gpui-pre 0.3.9 + Windows 上是空实现**，自己模拟拖动会
  完全没反应。拖动要用 `div().id(..).window_control_area(WindowControlArea::Drag)`，让
  系统返回 `HTCAPTION`。
- 拖动区的命中测试查的是 **`window.mouse_hit_test`（上一次已分发的鼠标事件位置）**，所以
  光标瞬移后立刻按下的那次可能判不到拖动区；真实鼠标会先移动，脚本注入才需要先补一次
  move。
- 拖动区里任何要接鼠标的子控件都必须 `.occlude()`（`HitboxBehavior::BlockMouse`）：命中
  测试按绘制顺序倒序收集 hitbox，遇到 BlockMouse 就截断，父级拖动区才不会跟着命中；
  不 occlude 就会「点了没反应，也不拖动」。
- List 不再用自带搜索框（`searchable(false)`），但 `ListState::set_query` 仍然可用（它写
  内部 input 并触发 `perform_search`）。它需要 `&mut Window`，所以只能经 `with_launcher`
  延迟调用。
- `Input` 根节点是 `size_full()`，宽度只能由外层 div 控制；输入框内边距是
  `Size::Medium` 的 `input_px() == 10` 两侧、`input_h() == 32`，测量文字宽度时要把它算进去。
- **`Entity::update_in` / `WeakEntity::update_in` 只对窗口根视图生效**，从只有 `&mut App`
  的回调里更新普通视图会报 `entity has no current window`。用 `with_launcher`。
- 在实体 A 的 update 里 update 实体 B 会 panic（`cannot update X while it is already being
updated`）。跨实体通知必须 `app.defer(...)`。
- 从回调和拦截器里更新窗口的错误不能 `.ok()` 吞掉：吞掉之后的现象和「按键完全没反应」
  一模一样。
- 创建窗口不要在自己的点击回调里直接开：`cx.defer` 到本周期之后再 hide + open，避免在
  窗口事件分发过程中挪窗口。

## 10. 开发要求

开始实现功能前，应先检查现有代码结构、依赖版本和相关模块，不要凭空假设已有 API。

修改代码时：

1. 保持现有架构的一致性，不引入当前用不到的抽象。
2. 只引入完成需求所必需的依赖。
3. 完成修改后执行 `cargo fmt --check`、`cargo clippy --all-targets`（退出码 0 不代表零
   warning，要读日志）、`cargo test`，并确认 release 构建通过。
