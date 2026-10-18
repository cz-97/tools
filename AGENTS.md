# AGENTS.md

## 1. 项目目标

一个基于 Rust 和 GPUI 的 Windows 原生启动器，个人自用。

核心交互：

1. 通过 Ctrl+Space 唤起启动器。
2. 输入关键词过滤命令。
3. 上下方向键选择。
4. Enter 执行选中项。
5. Esc 隐藏窗口；在设置页里 Esc 先返回命令列表。
6. 面板失去前台（点了别的窗口）后自动收起。
7. 拖动搜索栏空白处移动面板（纵坐标会被记住）。
8. 搜索栏右侧的齿轮图标打开面板内设置页（系统 / 外观 / 关于三个标签）。

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
- 设置页（面板内的 `Page::Settings`）与设置持久化（`src/settings.rs`）。
- 登录启动项的读写（`src/autostart.rs`，HKCU 下的 Run 键，无需提权）。
- 应用图标：`src/logo.rs` 画，`build.rs` 嵌进 exe（见 5.4）。
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
- 搜索框用 `Size::Small`（`input_px == 8`、`input_py == 2`、高 24）。Medium 的上下内边距
  是 8+8，一行 1.25rem 的文字只剩 16px，**下半截字形会被裁掉**；容器高度按设置里的
  `search_height`（默认 26）给足。Input 根节点是 `size_full().items_center()`，容器抬高
  时文字自动垂直居中，不必改字号。
- 设置按钮在搜索栏右端，点击把面板切到设置页（`Page::Settings`）；页头变成「返回 +
  设置」，同一个拖动区。

## 5.2 键盘

输入框是唯一持有焦点的控件，而 List 不再是焦点视图，所以**按键统一由
`App::intercept_keystrokes` 处理**（`enter` / `up` / `down` / `escape`），并通过
`with_launcher` 回到视图。`up/down` 按 `Page` 分派：命令页移动高亮，设置页切换标签；
`Esc` 在设置页先返回命令列表，再隐藏面板。拦截器用 `owns_window` 确认按键属于面板
窗口（现在只有这一个窗口，留着是给以后可能出现的窗口兜底）。

## 5.3 设置页

设置是面板内的一个页面（`Page::Settings(SettingsTab)`），不是独立窗口：左侧
系统 / 外观 / 关于三个标签（`SettingsTab`），右侧内容，页头是同一个拖动区。`h_flex`
默认在交叉轴居中，设置页的骨架要显式 `.items_stretch()`，否则内容会悬在面板中间。

每一项都是同一副骨架（`settings_row`）：**左侧是名称和它的描述（两行），右侧是控件**
（分段选择 `segmented`、步进 `stepper`、开关 `Switch`、只读值 `value_label`，或
「提示 + 箭头」的 `action_row`）。不要为某一项另起一套排布。

`h_flex` 交叉轴居中，控件靠 `justify_between` 推到右端；描述行 `min_w_0` +
`text_ellipsis`，控件 `flex_shrink_0`，所以描述宁可截断也不挤掉控件。

## 5.3 设置

设置存在 `%APPDATA%\gpui-tools\settings.ini`（`src/settings.rs`，`key = value` 文本，
坏行忽略、缺失取默认）：

- `theme` = `system` / `light` / `dark`。设置页改主题后立刻 `Theme::change` 并落盘。
- `window_y` = 面板顶边的物理像素。**只记纵坐标**，横坐标每次启动重新居中；恢复时按显示器
  工作区夹紧（`NativeWindow::center_x_at`）。写入时机：面板隐藏、退出命令、托盘退出。
- `search_height` = 搜索框高度（逻辑像素，24–56，步进 4，默认 26）。页头高度跟着它走
  （`header_height` = 高度 + 26，最小 52），所以调高时列表会被顶下去，这是预期的。
- `launch_at_login` = 是否写入登录启动项。**注册表才是事实**：启动时用
  `autostart::enabled()` 覆盖这一项，用户在任务管理器里删掉后不会被悄悄重建。
- `resume_seconds` = 子页面保持秒数（0 / 15 / 30 / 60 / 300）。`Launcher::remember_section`
  在**离开设置页时**（Esc 返回命令页、或面板隐藏）记下 `(时刻, 标签)`，
  `reset` 在窗口内重新唤起时回到那一页；超时就回命令页。Esc 在设置页是先返回命令页，
  所以离开的两条路径都要记，只记「隐藏」的话永远记不到设置页。

## 5.4 图标

`src/logo.rs` 是唯一的图形来源：纯计算的光栅化（4×4 超采样、多边形扫描线），
`rgba(size)` 给托盘，`ico()` 给可执行文件。`build.rs` 用 `#[path]` 直接包含它，
生成 `.ico` + `.rc`，找 `rc.exe`（PATH 或 `Windows Kits\10\bin\<最新>\x64`）编成 `.res`
再用 `cargo:rustc-link-arg-bins` 链进去；找不到 `rc.exe` 只警告、照常构建。

窗口图标（任务栏 / Alt-Tab）不归 GPUI 管，由 `NativeWindow::set_icon(1)` 用
`LoadImageW` 从 exe 的资源 id 1 取，失败写 stderr。改图形只动 `logo.rs`，
可执行文件、托盘、窗口三处一起变。

## 5.5 失焦隐藏

面板一旦被别的窗口抢走前台就收起（设置页也一样，它只是面板内的一页）。**GPUI 不给窗口
激活事件**，`Window::is_window_active` 也没有订阅口，所以 `Launcher::watch_focus` 用
`cx.spawn` 起一个循环，每 `FOCUS_CHECK_INTERVAL`(60ms) 问一次
`NativeWindow::is_foreground`（`GetForegroundWindow() == hwnd`）。

三条必须保留的细节：

- **连续 `FOCUS_LOST_SAMPLES`(3) 次**都没拿到前台才收，躲开「刚 ShowWindow、当前台还没
  转过来」的那一小段。
- **必须先成功前台过一次**（`was_in_front`）：`SetForegroundWindow` 失败时面板不该刚出现
  就消失，这时候宁可它不自动收。
- 循环在面板收起后 `break`，下一次唤起的 `reset` 重新 `spawn` 并顶掉旧的 Task。

## 6. 外部程序调用

- 调用外部程序一律使用参数数组（`Command::new(...).arg(...)`），不得把用户输入或任意文本
  拼接成 Shell 命令。
- **打开文件夹 / 链接一律走 `gpui_tools::shell::open`**（`ShellExecuteW`，verb `open`）：
  由 shell 解析关联，用户把默认文件管理器或浏览器换成别的也照样生效。**不要写
  `explorer.exe`、`cmd /c start`、`cx.open_url`**，那等于把用户的选择钉死在某一个程序上。
- 打开外部目标会让对方抢走前台，面板因此自动隐藏（见 5.5），这是预期行为，不要
  `cx.activate(true)` 把面板抢回来。

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
- `Input` 根节点是 `size_full()`，宽高都只能由外层 div 控制；内边距来自 `Size`：
  `input_px()`（Medium 10 / Small 8）与 `input_py()`（Medium 8 / Small 2），行高是
  1.25rem——容器高减去上下内边距必须 >= 行高，否则字形被裁。
- `h_flex()` = `flex().flex_row().items_center()`（gpui-base `styled.rs`），**交叉轴默认
  居中**；要顶对齐必须显式 `.items_stretch()` 或 `.items_start()`。`v_flex()` 只设
  `flex().flex_col()`，不装交叉轴对齐。
- **`Entity::update_in` / `WeakEntity::update_in` 只对窗口根视图生效**，从只有 `&mut App`
  的回调里更新普通视图会报 `entity has no current window`。用 `with_launcher`。
- 在实体 A 的 update 里 update 实体 B 会 panic（`cannot update X while it is already being
updated`）。跨实体通知必须 `app.defer(...)`。
- 从回调和拦截器里更新窗口的错误不能 `.ok()` 吞掉：吞掉之后的现象和「按键完全没反应」
  一模一样。

## 10. 开发要求

开始实现功能前，应先检查现有代码结构、依赖版本和相关模块，不要凭空假设已有 API。

修改代码时：

1. 保持现有架构的一致性，不引入当前用不到的抽象。
2. 只引入完成需求所必需的依赖。
3. 完成修改后执行 `cargo fmt --check`、`cargo clippy --all-targets`（退出码 0 不代表零
   warning，要读日志）、`cargo test`，并确认 release 构建通过。
