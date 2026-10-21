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
8. 搜索栏右侧的齿轮图标打开面板内设置页（系统 / 命令 / 外观 / 计算器 / 关于五个标签）。

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
- 每个命令的启用状态、调用次数与全局快捷键，以及唤出面板的热键（`src/shortcuts.rs` 的
  `Shortcuts` 全局）。
- 登录启动项的读写（`src/autostart.rs`，HKCU 下的 Run 键，无需提权）。
- 应用图标：`src/logo.rs` 画，`build.rs` 嵌进 exe（见 5.4）。
- 系统托盘。

release 构建是 **windows 子系统**（`windows_subsystem = "windows"`，见 `main.rs` 顶部），
双击或从快捷方式启动不会弹控制台、也不会一闪而过一个黑框。debug 构建保留控制台，
`eprintln!` 的诊断才看得见；release 下这些输出无处可去，属于预期。

### 亚克力背景

- 窗口背景外观由 `app::panel_appearance`（外观页的「半透明背景」开关）决定：开 =
  `WindowBackgroundAppearance::Blurred`，Windows 上 GPUI 映射成
  `ACCENT_ENABLE_ACRYLICBLURBEHIND`；关 = `Opaque`。
- **gpui-component 的 Root 插件每帧都会铺一层不透的 `theme.background`**，亚克力会被它
  盖成一片纯色。所以 `app::apply_root_background` 用 `gpui_kit::base::Root::update` 把
  Root 的 `style().bg` refin 成透明；面板自己的染色画在 `UiColors::panel` 上，深浅由
  设置 `panel_opacity`（30–100%，默认 78，外观页「透明度」一行）决定。
- Root 是 `open_window` **返回之后**才建好的，`Root::update` 不能在建窗闭包里调
  （panic：`window must have a Base Root`）。
- 换主题、翻「半透明」开关、系统主题变化（`observe_global::<Theme>`）都要同时重刷
  `apply_panel_appearance` 与 `apply_root_background`。
- 参照实验：`src/bin/theme_probe.rs`（`cargo run --bin gpui-theme-probe`）四扇窗对照
  raw/root × light/dark；`src/bin/transparency_probe.rs` 是最早的那扇。
- **验证亚克力只能看屏幕合成结果**：`target/cap.py`（屏幕 DC BitBlt）行，
  `crop.py`（PrintWindow）只画窗口自己那层，半透明区域会变纯黑/白。

### 配色（0.11.0 起：烟熏玻璃）

- **配色只有一份表**：`src/ui/colors.rs` 的 `DARK` / `LIGHT`，值是 `0xRRGGBBAA`。
  块、分隔线、选中态全部是**纱**（暗色主题叠白纱、浅色主题叠黑纱），不是实心色——
  纱才会让亚克力从块里透上来，实心色在半透明面板上就是一块补丁。
- **面板浓度有下限 `FROST_FLOOR`(0.72)**：外观页的 30% 映射到这里，100% 才是全实。
  低于这个浓度，浅色壁纸会把面板冲成中灰，白字怎么调都发虚。
- **不要再回到「按壁纸极端颜色算对比度」那条路**（0.9.2–0.10.2 走过）：面板一旦落到
  中灰，白底要求字更黑、黑底要求字更亮，目标值数学上不存在，结果只能是发脏的中间色。
  正确做法是钉住面板浓度、把文字颜色写死。
- 面板外沿的 1px 高光边（`UiColors::panel_edge`）是「玻璃」的关键暗示，别删。
- 主题色（`Theme`）只写基础色，供开关、滑动条这些组件用；`UiColors` 在它之上加面板浓度。

## 4. 命令

命令全部声明在 `src/catalogue.rs` 的 `COMMANDS` 里。一条 `Command` = `id` + `section` +
名称 + 描述 + 搜索词 + 图标 + `fn(&mut App)` 动作。

**`section` 就是设置里的子模块**（`Section` = 系统 / 命令 / 外观 / 计算器 / 关于，与设置左侧的
五个标签一一对应）：命令归到它作用的那一页下面。现有的几条（设置 / 计算器 / Program Folder /
Repository / Quit GPUI Tools）都是启动器自身的事，所以都属于「系统」。

新增命令 = 往 `COMMANDS` 里加一项，选一个 `section`。**不需要动设置页、主页列表或存储
代码**：搜索、列表渲染、命令页分组、启用开关、调用次数、全局快捷键全部由 `id` 派生。
新增子模块 = 给 `Section` 加一个变体（同时它也就是一个新的设置标签）。

- `Command::id` 是持久化主键（数据库、计数、快捷键都按它存），一旦发布就**不要改**，
  改名只动 `name`。id 必须全表唯一。
- 不想出现在主页的命令不删代码，而是在命令页关掉（`enabled = false`）。
- 需要窗口的命令走 `with_launcher`，避免在按键分发过程中重入窗口。

## 5. 列表 UI 约定

列表项布局：

- 左侧：图标。
- 中间第一行：名称；第二行：描述。
- 右侧（始终显示，不随是否有值而变）：所属子模块、调用次数、全局快捷键（没有时写
  「无快捷键」）。三项都读 `CommandState` 快照，不从全局现取。

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
**计算器行里有焦点时，`up/down/enter` 一律放行**（`app::calculator_typing`）：那三个键
在公式框里分别是移动光标和换行，被设置页的「切换子模块」抢走就没法输入了。

## 5.3 设置页

设置是面板内的一个页面（`Page::Settings(SettingsTab)`），不是独立窗口：左侧
系统 / 命令 / 外观 / 计算器 / 关于五个标签（`SettingsTab`），右侧内容，页头是同一个拖动区。
`h_flex` 默认在交叉轴居中，设置页的骨架要显式 `.items_stretch()`，否则内容会悬在面板中间。

每一项都是同一副骨架（`settings_row`）：**左侧是名称和它的描述（两行），右侧是控件**
（分段选择 `segmented`、步进 `stepper`、开关 `Switch`、只读值 `value_label`，或
「提示 + 箭头」的 `action_row`）。不要为某一项另起一套排布。

`h_flex` 交叉轴居中，控件靠 `justify_between` 推到右端；描述行 `min_w_0` +
`text_ellipsis`，控件 `flex_shrink_0`，所以描述宁可截断也不挤掉控件。

## 5.3.1 计算器子模块

「计算器」是一个普通子模块（`Section::Calculator`，键 `calculator`），内容在
`src/ui/calculator_page.rs`；主页上另有一条同名命令直接把它唤出。

- **求值在 `src/calculator.rs`**：递归下降，只认 `+ - * / ^` 五种运算，外加括号分组、
  小数、一元正负号；`^` 右结合且低于一元负号（`-2^2 = -4`、`2^-1 = 0.5`）。全角符号
  （`＋－＊／＾（）．` 和全角数字）与空白一律先归一化。除零、打了一半、溢出、NaN
  都算「算不出」，返回 `None`，界面就不画结果——不要为了报错另设一套错误态。
- **结果是从公式现算的**，不另存一份：公式每改一次（`InputEvent::Change`）重画一次即可。
- **行高自适应**靠 `TextareaState::auto_grow(1, 8)`：长公式自动换行、自动长高，
  最多 8 行，再多横着看。
- **最后一行永远是空行**：它有内容就在下面追一个（`Launcher::append_calculator_row`）。
  建新行要窗口，而事件回调里只有视图，所以走 `with_launcher` 推到下一拍。
- 行用**稳定 id**（`CalculatorRow::id`）而不是下标：行会增删，下标会挪窝，
  订阅和「清空本行」记的下标会对不上行。
- **点结果复制**（`copy_calculator_result`）：复制后那一行亮 1.2 秒「已复制」，
  到点由 `cx.spawn` 的定时器收回去。
- **清空本行**清的是用户最后一次点进去的那行（`InputEvent::Focus` 记的 `current_row`），
  清完焦点还留在那一行；**清空所有**回到「只有一行空行」的初始状态。两个按钮在页面底部。
- 计算器的行**不落库**：面板收起再唤起还在，重启才没。别为它改 `settings` 表结构。

## 5.3 设置

设置存在 SQLite 数据库 `%APPDATA%\gpui-tools\gpui-tools.db`（`src/settings.rs`，
`rusqlite` 的 `bundled` 特性）。两张表，整份记录一次读、一次写（事务）：

```sql
CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE commands (section TEXT NOT NULL, id TEXT NOT NULL,
                       enabled INTEGER NOT NULL, uses INTEGER NOT NULL,
                       shortcut TEXT, PRIMARY KEY (section, id));
```

- `settings` 里的键：`theme`(system/light/dark)、`window_y`（面板顶边物理像素，只记纵坐标）、
  `search_height`（24–56，页头高度 = 高度 + 26、最小 52）、`launch_at_login`（**注册表才是
  事实**，启动时覆盖）、`resume_seconds`（子页面保持秒数）、`summon_shortcut`（唤出面板的
  热键，值同下面的 `shortcut`；写 `none` 表示**用户明确不绑**，只能从托盘打开——这一行
  不能读成默认值）。
- **`commands` 的主键是 `(section, id)`**：`section` = `Section::key()`（`system` / `commands` /
  `appearance` / `about`），两个子模块可以有同名的命令而互不干扰。`Section::key()` 与
  `Command::id` 都是持久化主键的一半，**发布后都不要改**（改标签不影响，改 key 会丢数据）。
- **只有用户改过**的命令才有行（全默认的不写），所以表不会随命令变多而膨胀。
- 读不出来（文件损坏 / 打不开）就退回默认并写 stderr，绝不让启动器起不来。
- **不写迁移**：这是自用项目，表结构一改就直接删库重建（`%APPDATA%\gpui-tools\gpui-tools.db`），
  不要保留 `PRAGMA user_version` 分支、旧格式解析或 legacy 字段。老版本留下的
  `settings.toml` / `settings.ini` 也不再读。
- `uses` 只在内存里累加，面板隐藏或退出时随整份记录落盘（`remember_window_position`）。
  所以那个函数**必须每次隐藏都写**，不能「window_y 没变就早退」，否则计数永远存不下来。
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

「命令」标签里再分左右：左侧是**设置子模块**（`catalogue::Section::ALL`：系统 / 命令 /
外观 / 关于），右侧是该子模块的命令行。列表上方写「<子模块>的命令」，空组显示
「这个子模块还没有命令」。
每行：图标 + 名称/描述 + 调用次数 + 快捷键按钮 + 启用开关。左侧每项右侧显示
「启用数/总数」。

- 点快捷键按钮进入**录制**状态（`Launcher::recording`，类型是 `RecordTarget`：某条命令，
  或者唤出面板的那个热键），下一组按键就是答案：`Esc` 取消、`Backspace`/`Delete` 清除、
  没有修饰键的组合被忽略（否则会把面板里打字的按键当成全局快捷键）。**右键按钮直接清除**
  （命令页和系统页的「全局快捷键」共用 `commands_page::shortcut_button`，两处交互必须一致）。
  录制期间 `intercept_keystrokes` **吞掉所有按键**，否则字符会漏进隐藏着的搜索框。
- **绑之前先查冲突**（`shortcuts::owner_of_chord`，唤出热键 + 所有命令的设置值一起看）：
  这组键已经归别人就不绑、不覆盖，按钮上写「已被 X 占用」，录制继续等下一组键。
- **录制期间全局热键一律不执行**（`app::recording_shortcut`）：注册过的键会被系统从焦点
  窗口截走，按键到不了 `capture_shortcut`，冲突是从热键事件那条路上报回来的
  （`Launcher::report_conflict`）。不拦住的话，想录的键会跑去执行别人的命令。
- **面板隐藏时必须结束录制**（`Launcher::leave`）：录制状态留着的话，热键分发会一直当
  「在录制」而全部静默失效，连唤出面板的那一个都按不回来。
- 唤出热键和命令热键同源：`settings.summon_shortcut` + `apply_summon_shortcut`，可以改、
  可以不绑（托盘兜底）；`HotKey` 反解不回字符串，所以 `Shortcuts` 里连组合键一起存。
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
- `shell::open` **在自己的线程里跑**（先 `CoInitializeEx` 成 STA）。原因：`ShellExecuteW`
  会起 shell 自己的消息泵，泵在 GPUI 持有 App 借用时把事件送回面板，就会
  `RefCell already borrowed` → `panic in a function that cannot unwind` → 进程直接 abort
  （实测在列表点击里打开文件夹必崩）。**绝不要**在 `&mut App` 的回调里直接调
  `ShellExecuteW`。
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
   warning，要读日志）、`cargo test`。
