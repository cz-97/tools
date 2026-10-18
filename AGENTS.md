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
8. 搜索栏右侧的齿轮图标打开面板内设置页（系统 / 命令 / 外观 / 关于四个标签）。

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
- 设置页（面板内的 `Page::Settings`）与设置持久化（`src/settings.rs`，TOML）。
- 命令目录（`src/catalogue.rs`）：模块、命令、搜索词与动作都声明在那里。
- 每个命令的启用状态、调用次数与全局快捷键（`src/main.rs` 的 `Shortcuts` 全局）。
- 登录启动项的读写（`src/autostart.rs`，HKCU 下的 Run 键，无需提权）。
- 应用图标：`src/logo.rs` 画，`build.rs` 嵌进 exe（见 5.4）。
- 系统托盘。

## 4. 命令

命令全部声明在 `src/catalogue.rs`：`MODULES` 是模块（如「文件」「链接」「应用」）的数组，
每个模块带自己的 `commands`。一条 `Command` = `id` + 名称 + 描述 + 搜索词 + 图标 +
`fn(&mut App)` 动作。

新增命令 = 往某个模块的 `commands` 里加一项；新增模块 = 加一个 `Module`。
**不需要动设置页、主页列表或存储代码**：搜索、列表渲染、命令页、启用开关、调用次数、
全局快捷键全部由 `id` 派生。

- `Command::id` 是持久化主键（设置文件、计数、快捷键都按它存），一旦发布就**不要改**，
  改名只动 `name`。id 必须全表唯一。
- 不想出现在主页的命令不删代码，而是在命令页关掉（`enabled = false`）。
- 需要窗口的命令走 `with_launcher`，避免在按键分发过程中重入窗口。

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
系统 / 命令 / 外观 / 关于四个标签（`SettingsTab`），右侧内容，页头是同一个拖动区。`h_flex`
默认在交叉轴居中，设置页的骨架要显式 `.items_stretch()`，否则内容会悬在面板中间。

每一项都是同一副骨架（`settings_row`）：**左侧是名称和它的描述（两行），右侧是控件**
（分段选择 `segmented`、步进 `stepper`、开关 `Switch`、只读值 `value_label`，或
「提示 + 箭头」的 `action_row`）。不要为某一项另起一套排布。

`h_flex` 交叉轴居中，控件靠 `justify_between` 推到右端；描述行 `min_w_0` +
`text_ellipsis`，控件 `flex_shrink_0`，所以描述宁可截断也不挤掉控件。

## 5.3 设置

设置存在 `%APPDATA%\gpui-tools\settings.toml`（`src/settings.rs`，`serde` + `toml`）。
缺字段取默认、未知字段忽略、坏文件整体回退默认；老的 `settings.ini` 只在第一次读到
TOML 之前迁移一次（`Settings::adopt_legacy`），之后不再看它。

```toml
theme = "system"          # system / light / dark
window_y = 422            # 面板顶边物理像素，只记纵坐标
search_height = 26        # 24–56，页头高度 = 高度 + 26（最小 52）
launch_at_login = false   # 注册表才是事实，启动时覆盖
resume_seconds = 0        # 子页面保持秒数

[commands.program-folder] # key = Command::id
enabled = false           # 是否出现在主页
uses = 3                  # 调用次数
shortcut = "ctrl+alt+KeyP" # 可选，全局快捷键
```

- 只有**用户改过**的命令才写进文件：全默认的记录在保存时被 `prune` 掉，所以文件不会随着
  命令变多而膨胀。
- `uses` 只在内存里累加，面板隐藏或退出时随整份记录落盘（`remember_window_position`）。
  所以那个函数**必须每次隐藏都写**，不能再「window_y 没变就早退」，否则计数永远存不下来。
- `shortcut` 存的是 `keyboard-types` 的键名（`KeyP` / `Digit1` / `F1` / `ArrowUp`），
  显示时才转成 `Ctrl + Alt + P`。解析失败只影响那一条绑定。
- `window_y` 写入时机：面板隐藏、退出命令、托盘退出；恢复时按显示器工作区夹紧
  （`NativeWindow::center_x_at`）。
- `resume_seconds`：`Launcher::remember_section` 在**离开设置页时**（Esc 返回命令页、
  或面板隐藏）记下 `(时刻, 标签)`，`reset` 在窗口内重新唤起时回到那一页；超时回命令页。
  Esc 在设置页是先返回命令页，所以两条离开路径都要记，只记「隐藏」永远记不到设置页。

## 5.4 图标

`src/logo.rs` 是唯一的图形来源：纯计算的光栅化（4×4 超采样、多边形扫描线），
`rgba(size)` 给托盘，`ico()` 给可执行文件。`build.rs` 用 `#[path]` 直接包含它，
生成 `.ico` + `.rc`，找 `rc.exe`（PATH 或 `Windows Kits\10\bin\<最新>\x64`）编成 `.res`
再用 `cargo:rustc-link-arg-bins` 链进去；找不到 `rc.exe` 只警告、照常构建。

窗口图标（任务栏 / Alt-Tab）不归 GPUI 管，由 `NativeWindow::set_icon(1)` 用
`LoadImageW` 从 exe 的资源 id 1 取，失败写 stderr。改图形只动 `logo.rs`，
可执行文件、托盘、窗口三处一起变。

## 5.5 命令页

「命令」标签里再分左右：左侧是模块（`catalogue::MODULES`），右侧是选中模块的命令行。
每行：图标 + 名称/描述 + 调用次数 + 快捷键按钮 + 启用开关。模块列的右侧显示
「启用数/总数」，右侧列表上方是模块的 `description`。

- 点快捷键按钮进入**录制**状态（`Launcher::recording`），下一组按键就是答案：
  `Esc` 取消、`Backspace`/`Delete` 清除、没有修饰键的组合被忽略（否则会把面板里打字的
  按键当成全局快捷键）。录制期间 `intercept_keystrokes` **吞掉所有按键**，否则字符会漏进
  隐藏着的搜索框。
- 键名用 `keyboard-types` 的 `Code`（`KeyP`），显示时才友好化。`chord_from_keystroke`
  只认字母、数字、`f1..f24` 和一张命名键表，其它键直接不响应。
- 改快捷键后 `apply_command_shortcuts` **全部注销再全量注册**：集合很小，局部更新反而要
  记「哪个键属于哪条命令」，注册失败的只跳过那一条。
- 快捷键命中后走 `run_command`（计数 + 执行），和列表里按 Enter 是同一条路径。

## 5.6 失焦隐藏

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
- 打开外部目标会让对方抢走前台，面板因此自动隐藏（见 5.6），这是预期行为，不要
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
