# AGENTS.md

Rust + GPUI 的 Windows 启动器，个人自用。这份文档是唯一的项目规范：先读它，再动手。

## 1. 项目目标

一个快捷键唤出的搜索面板：输入关键词找命令、算数、转金额、开网址/路径。

核心交互：

1. `Ctrl+Space` 唤出面板（可在设置里改或解绑，托盘兜底）。
2. 输入关键词过滤命令；命中「指令匹配」时结果排在最前面。
3. `↑` / `↓` 选择，`Enter` 执行（复制结果或打开目标）。
4. `Esc` 隐藏；在设置/计算器页里 `Esc` 先退回命令页。
5. 面板失去前台（点了别的窗口）自动收起。
6. 拖动搜索栏空白处移动面板（记住纵坐标）。
7. 搜索栏右侧齿轮打开面板内设置。

不追求通用启动器框架，不做插件体系。命令是编译进主程序的静态表。

## 2. 技术原则

- Rust + GPUI，不引入 Electron / WebView。
- 依赖要轻、职责要单一。
- 不为抽象而抽象：不提前造扩展体系、SDK、清单格式或进程协议。
- 阻塞操作不得放在 UI 线程。

## 3. 源码结构

`lib` 放宿主侧服务（纯逻辑 + 平台细节），`bin` 放面板与界面。`lib.rs` / `main.rs` 用
`pub use` 把子目录里的模块重新导出到顶层，所以旧的 `gpui_tools::settings`、
`crate::catalogue` 这类路径仍然有效——挪文件不必改调用点。

```text
src/
  main.rs                 bin 入口：mod 声明 + 顶层 re-export
  lib.rs                  lib 入口：logic / platform + 顶层 re-export
  strings.rs              所有界面文案（改文案只动这里）
  app/                    应用外壳
    mod.rs                启动流程、全局状态、面板显隐、按键拦截、with_launcher
    actions.rs            命令动作的具体实现
    tray_theme.rs         托盘图标与菜单配色
  command/                命令与热键
    catalogue.rs          命令静态表 COMMANDS、Section
    shortcuts.rs          全局热键注册 / 冲突检测
  logic/                  纯逻辑，可单测
    calculator.rs         表达式求值
    cache.rs              网络结果的文件缓存
    firefox.rs            读火狐系浏览器的 places.sqlite（书签 / 历史 / 图标）
    github.rs             GitHub 仓库查询（个人项目 / 星标 / 搜索）
    matcher.rs            指令匹配（算式 / 金额 / 网址 / 路径）
    settings.rs           SQLite 持久化
  platform/               Windows 平台细节
    autostart.rs          HKCU Run 开机启动
    instance.rs           单例（命名互斥量 + 唤醒事件）
    logo.rs               纯计算图标光栅化
    shell.rs              ShellExecuteW 打开外部目标
    window.rs             无边框窗口外壳
  ui/                     界面
    mod.rs
    colors.rs             配色表 DARK / LIGHT
    commands_list.rs      主页列表（命令 + 指令匹配）及其 ListDelegate
    launcher/
      mod.rs              面板状态与行为：导航、换页、动作
      view.rs             绘制：页头、页脚、整页骨架、impl Render
    pages/
      commands.rs         设置里的「命令」页
      settings.rs         设置页各标签
      calculator.rs       计算器一级页面
      lists.rs            工具列表页（书签 / 历史 / 个人项目 / 星标 / 查询仓库）
```

## 4. 主要模块

### 4.1 面板与页面（`ui/launcher`）

页面状态是 `Page`：

```text
Page::Commands           主页：搜索框 + 列表
Page::Settings(SettingsTab)  设置（系统/命令/外观/工具/指令匹配/关于）
Page::Calculator         计算器一级页面
Page::List(ListKind)     工具列表页（书签/历史/个人项目/星标/查询仓库）
```

- 行为在 `launcher/mod.rs`，绘制在 `launcher/view.rs`；两边用同一个 `Launcher` 视图。
- 需要窗口的操作（换页、聚焦输入框、改列表选中）都在拿到 `&mut Window` 的地方做；
  只有 `&mut App` 的回调一律走 `with_launcher`（延后到下一拍，避免重入）。
- 页面状态和设置存在 `Launcher` 上；不要把可变的临时状态塞进全局。

### 4.2 设置页（`ui/pages/settings.rs`）

设置是面板内的一页，不是独立窗口。左侧标签是 `SettingsTab`，**注意它和命令页的
`catalogue::Section` 已经不是一回事**：

- `SettingsTab`：系统 / 命令 / 外观 / **工具** / **指令匹配** / 关于（设置左侧导航）。
- `Section`：系统 / 命令 / 外观 / **浏览器** / **GitHub** / 计算器 / 关于（命令页给命令分组）。

每一项都是同一副骨架 `settings_row`：左侧名称 + 描述（两行），右侧控件
（`segmented` / `stepper` / `Switch` / `value_label` / `action_row`）。不要另起排布。

`h_flex` 交叉轴默认居中；设置页骨架要显式 `.items_stretch()`。描述行 `min_w_0` +
`text_ellipsis`，控件 `flex_shrink_0`：宁可截断描述，也不挤掉控件。

**「工具」是容器标签**：它自己不画设置项，而是把工具收在子标签页里（`ToolTab`：计算器 /
火狐系浏览器 / GitHub），左右两栏布局照命令页：左列子标签、右列内容。加工具 = 给
`SettingsTab::Tools` 下的 `ToolTab` 加一项。

工具页里的文本框（火狐 profile 目录、GitHub 用户名和令牌）**失焦或回车时保存**
（`Launcher::commit_settings_inputs`），不设「保存」按钮；换标签、回主页也会先存一次。

**「指令匹配」标签**：四个匹配各一行开关，见 4.4。

### 4.3 计算器

计算器是**和设置平级的一级页面**（`Page::Calculator`），主页上有同名命令直接进入；
它的设置（结果精度）放在「设置 → 工具 → 计算器」。

- 求值在 `logic/calculator.rs`：递归下降，只认 `+ - * / ^`、括号、小数、一元正负号；
  `^` 右结合且低于一元负号（`-2^2 = -4`、`2^-1 = 0.5`）。全角符号与空白先归一化。
  除零、打了一半、NaN 都返回 `None`——界面不画结果，不要另设错误态。
- 结果**从公式现算**，不另存：`InputEvent::Change` 时重画。
- 行用 `TextareaState::auto_grow(1, 8)` 自动换行长高；**最后一行永远是空行**，有内容就在
  下面补一行（建新行要窗口，走 `with_launcher`）。
- 行用**稳定 id**（`CalculatorRow::id`）不用下标：行会增删，下标会挪。
- 点结果复制并亮 1.2 秒「已复制」（`COPY_FEEDBACK`）；清空本行清的是最后点进去的那行
  （`InputEvent::Focus` 记的 `current_row`）。计算器的行**不落库**。

### 4.4 指令匹配（`logic/matcher.rs`）

搜索框输入先当「指令」解析，命中就排在列表最前面。输入 → `matcher::matches(query, config)`
→ `Vec<Match>`。`Match` 有三段：`kind`（选图标）、`title` / `description`、`action`。

两类动作（`MatchAction`）：

- `Copy(text)`：显示结果，点击/回车复制（算式结果、金额大写）。
- `Open(target)`：执行动作，交给 `shell::open`（网址、路径）。
- `None`：只提示（不存在的绝对路径）。

四个匹配（开关存 `Settings::matcher_*`，设置页「指令匹配」标签逐项控制）：

| 匹配 | 命中条件 | 结果 |
| --- | --- | --- |
| 算式 | 含运算符且能求值（裸数字不算，避免和金额撞车） | 显示结果，复制 |
| 金额大写 | 最多两位小数的纯数字（可带 `¥`） | 中文大写，复制 |
| 网址 | `http(s)://`、`www.`、`localhost`，或常见 TLD 的域名 | 补全协议后打开 |
| 路径 | 盘符/UNC/`~`/含分隔符；存在则打开，绝对路径不存在则提示 | 打开 / 提示 |

避坑：

- 算式要求含运算符；否则 `123.45` 会同时被「算式」和「金额」命中。
- `1/2` 只算算式（0.5），不当路径。
- 带 `://` 的不当路径；`config.toml` 这类后缀不在常见 TLD 表里，不当网址。
- 相对路径不存在时静默，只有绝对路径才报「不存在」。

主页列表的项是 `HomeItem`（匹配或命令），渲染成 `HomeRow`。匹配排在命令前面，和命令
一起参与 `↑↓`、`Enter`、点击。点/回车后动作走 `Launcher::activate_match`，做完把焦点
还给搜索框（点列表会先夺走焦点）。

### 4.5 工具列表页（`ui/pages/lists.rs`）

五个页面共用一张列表和一份 delegate（`EntryDelegate`），页头、空状态、行排版按
`ListKind` 分派：

| `ListKind` | 入口命令 | 数据 | 行 |
| --- | --- | --- | --- |
| `Bookmarks` | 书签 | `firefox::bookmarks`，每次进来重读 | 单行：名字 + 网址 |
| `History` | 历史 | `firefox::history`（最多 500 条） | 单行：名字 + 网址 + 次数·时间 |
| `Projects` | 个人项目 | `github::owned_repos`，**缓存优先** | 双行：名字/语言 + ★数·简介 |
| `Stars` | 星标 | `github::starred`，**缓存优先** | 同上 |
| `Search` | 查询仓库 | `github::search`，**输入即搜** | 同上 |

页头从左到右：返回、图标、标题、搜索框（空着时宽度按提示文字量，不能只剩半个字）、
`flex_1`、**右上角那枚按钮**（书签/历史/仓库是「重新读取」，查询仓库是「再搜一次」）。

- **搜索**：本地页（前四个）输入即过滤，不过滤也可以看；`Search` 页不做本地过滤
  （`language:rust` 这类限定符会被滤没），改成 600ms 防抖后重新请求（`SEARCH_DEBOUNCE`，
  流水号 `search_serial` 作废旧的那次）。
- **焦点**：列表页的默认焦点永远是搜索框；`Tab` 在搜索框和右上角按钮之间切换
  （`toggle_list_action`，切到按钮时 `window.blur`，按钮加底色）。点完行、点完按钮
  都要把焦点还给搜索框。
- **点击**：行自己挂 `on_click`（`ListState` 建的时候 `selectable(false)`，不然
  `List` 的 `confirm` 分不出单击双击）。**单击** = 选中 + 复制链接，**双击** = 打开。
- 行是 `Entry`：标题 / 描述 / 右侧元信息 / 打开目标 / 站点图标。目标一律交给
  `shell::open`，**不要**在 UI 回调里直接 `ShellExecuteW`。
- 缓存优先：进页面先读 `%APPDATA%\gpui-tools\cache\github-{projects|stars}-{user}.json`，
  有就直接摆出来、**不发请求**；没有才抓一次。右上角那枚按钮才是强制走网络。
- 阻塞的读库和联网都走 `cx.background_executor().spawn`（网络用 `platform/http.rs` 的
  WinINet，不用 GPUI 的 `cx.http_client()`：桌面端那是空实现）；两者都装在
  `Launcher::pending` 里，换页/收起时丢掉这个 Task，结果就不会串页。
- 书签和历史的图标：`moz_icons` **整表**读一遍，按 `id` 和按域名（`icon_url` 的 host，
  去 `www.`）各建一份索引；取图标先按 `moz_pages_w_icons` 关联到的 icon id，找不到再按
  页面域名兜底。只靠关联那一份会有一半书签没图标（那正是第一版的毛病）。
  ICO / PNG / SVG 都能画（SVG 走 `svg_renderer`）；认不出的格式退回 `Entry::tile`。

### 4.6 命令目录与命令页

命令全部声明在 `command/catalogue.rs` 的 `COMMANDS`：`id` + `section` + 名称 + 描述 +
搜索词 + 图标 + `fn(&mut App)`。加命令 = 加一项；搜索、列表、命令页分组、开关、次数、
快捷键全由 `id` 派生，不用动别处。

- `Command::id` 是持久化主键的一半，**发布后不要改**；改名只动 `name`。
- 不想上主页的命令不删代码，在命令页关掉（`enabled = false`）。
- 需要窗口的动作走 `with_launcher`，不要在按键分发里重入窗口。

命令页（设置 → 命令）左右两栏：左列 `catalogue::Section::ALL`，右列该分组的命令行。
每行：图标 + 名称/描述 + 调用次数 + 快捷键按钮 + 启用开关；左列每项显示「启用数/总数」。

### 4.7 快捷键

- 左键录制 / 右键清除（`command/shortcuts.rs` + `ui/pages/commands.rs` 的
  `shortcut_button`，命令页与系统页共用）。
- 录制期间 `intercept_keystrokes` 吞掉所有按键；`Esc` 取消、`Backspace`/`Delete` 清除、
  无修饰键忽略。
- **冲突不靠拦截判**：`RegisterHotKey` 会把组合键从焦点窗口截走，冲突从热键事件那条路
  报回（`report_conflict`）。**面板隐藏必须结束录制**（`leave`），否则所有热键静默失效。
- 唤出热键和命令热键同源：`settings.summon_shortcut`；`none` 表示明确不绑（托盘兜底）。
- 键名用 `keyboard-types` 的 `Code` 存，显示时才友好化。

## 5. 交互细节

### 5.1 搜索栏与拖动

搜索栏是面板唯一的拖动面，用 `WindowControlArea::Drag` 交给系统（不要自己模拟拖动）。

- 搜索栏容器整体是拖动区；**需要鼠标的控件必须 `.occlude()`**，否则按下会被当成拖标题栏。
- 搜索框宽度跟随文字（`search_field_width`），不用 `flex_1`：多占一像素就少一像素可拖。
- 占位提示是拖动区里的普通 `div`，不是 Input 的 placeholder。
- 搜索框用 `Size::Small`（`input_py = 2`）；Medium 的 8+8 内边距会裁掉字形。容器高度按
  `settings.search_height` 给足，Input 根节点 `size_full().items_center()` 会自动居中。

### 5.2 键盘

输入框是唯一持有焦点的控件，List 不是焦点视图，所以按键统一在 `App::intercept_keystrokes`
里处理（`enter` / `up` / `down` / `escape`），经 `with_launcher` 回到视图：

- `up/down`：命令页移动高亮，设置页切换标签。
- `enter`：执行高亮项（`activate_selected`，分派匹配或命令）；列表页里执行页头按钮
  （焦点在按钮上时）或打开高亮那一行。
- `tab`：只在列表页有用——搜索框和右上角按钮之间换焦点（见 4.5）。
- 列表页的搜索框常驻焦点，`↑↓` 仍由拦截器收走做选中移动，不进输入框。
- `escape`：设置/计算器页先回命令页，否则收面板。
- **计算器行有焦点时 `up/down/enter` 一律放行**（`app::calculator_typing`），那三个键在公式
  框里是光标移动和换行。
- 拦截器用 `owns_window` 确认按键属于面板窗口。

### 5.3 页面保持（resume）

`resume_seconds` > 0 时，收起面板后短时间内再唤出会回到原页面：

- `Launcher::remember_page` **只在收起那一刻**按当前页记：设置/计算器就记，命令页就
  **清空**记忆。
- `Esc` 退回命令页本身不记页面——既然先回到了主页，再收起就不该又被送回设置。
- `reset` 按记忆换页；超时或没记忆就回命令页。

### 5.4 失焦隐藏

面板被别的窗口抢走前台就收起。**GPUI 不给窗口激活事件**，所以 `Launcher::watch_focus` 用
`cx.spawn` 每 `FOCUS_CHECK_INTERVAL`(60ms) 问一次 `NativeWindow::is_foreground`：

- 连续 `FOCUS_LOST_SAMPLES`(3) 次拿不到前台才收。
- **必须先前台成功过一次**（`was_in_front`），避免刚出现就被收。
- 面板收起后循环 `break`，下次 `reset` 重新 `spawn` 顶掉旧 Task。

## 6. 外观

### 6.1 亚克力

- 背景外观由 `app::panel_appearance`（外观页「半透明背景」）决定：开 = `Blurred`（Windows
  映射成 acrylic），关 = `Opaque`。
- **gpui-component 的 Root 每帧铺一层不透的 `theme.background`**，会盖死亚克力；
  `app::apply_root_background` 用 `Root::update` 把它 refin 成透明。Root 在 `open_window`
  **返回之后**才存在，建窗闭包里调会 panic（`window must have a Base Root`）。
- 换主题 / 翻半透明 / 系统主题变化都要同时重刷 appearance 与 root 背景。

### 6.2 配色

- 配色只有一份表：`ui/colors.rs` 的 `DARK` / `LIGHT`，值 `0xRRGGBBAA`。
- 块、分隔线、选中态是**纱**（暗色叠白纱、浅色叠黑纱），不是实心色——纱才让亚克力透上来。
- 面板浓度有下限 `FROST_FLOOR`(0.72)；低于它浅色壁纸会把面板冲成中灰。
- 不要回到「按壁纸极端颜色算对比度」那条路，钉住浓度、把字色写死。
- 面板外沿 1px 高光边（`UiColors::panel_edge`）是玻璃感的关键，别删。
- **验证亚克力只能看屏幕合成**：BitBlt / PrintWindow 抓不到 DWM 合成内容，靠用户看。

## 7. 图标

`platform/logo.rs` 是唯一图形来源：纯计算光栅化，`rgba(size)` 给托盘，`ico()` 给 exe。
`build.rs` 用 `#[path = "src/platform/logo.rs"]` 包含它，生成 `.ico` + `.rc`，找 `rc.exe`
编成 `.res` 链进去；找不到 `rc.exe` 只警告、照常构建。窗口图标由
`NativeWindow::set_icon(1)` 从 exe 资源 id 1 取。改图形只动 `logo.rs`。

## 8. 设置持久化（`logic/settings.rs`）

SQLite：`%APPDATA%\gpui-tools\gpui-tools.db`，`rusqlite` 的 `bundled`。两张表，整份一次
事务读、一次写：

```sql
CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE commands (section TEXT NOT NULL, id TEXT NOT NULL,
                       enabled INTEGER NOT NULL, uses INTEGER NOT NULL,
                       shortcut TEXT, PRIMARY KEY (section, id));
```

- `settings` 键：`theme`、`translucent`、`panel_opacity`、`window_y`、`search_height`、
  `launch_at_login`（**注册表才是事实**）、`resume_seconds`、`summon_shortcut`（`none` =
  明确不绑）、`calculator_precision`、`matcher_calculator` / `matcher_amount` /
  `matcher_url` / `matcher_path`、`firefox_profile`（缺这一行 = 自动探测 profile）、
  `github_user`、`github_token`。
- 网络结果不进这张库：它们存在 `%APPDATA%\gpui-tools\cache\<键>.json`（见 `logic/cache.rs`），
  丢了从网上再抓一次就有。
- `commands` 主键 `(section, id)`；只有用户改过的命令才有行。
- 读不出来就退回默认并写 stderr，绝不让程序起不来。
- **不写迁移**：自用项目，改表结构直接删库重建，别留 `PRAGMA user_version` 分支。
- `uses` 在内存累加，收起面板时随整份记录落盘——`remember_window_position` **必须每次
  隐藏都写**，不能「window_y 没变就早退」。

## 9. 外部程序调用

- 调外部程序一律用参数数组（`Command::new(..).arg(..)`），不拼 Shell 命令。
- **打开文件夹 / 链接一律 `gpui_tools::shell::open`**（`ShellExecuteW`，verb `open`）：
  由 shell 解析关联。不要写死 `explorer.exe`、`cmd /c start`、`cx.open_url`。
- `shell::open` 在**自己的线程**里跑（先 `CoInitializeEx` 成 STA）：`ShellExecuteW` 会起
  shell 消息泵，泵在 GPUI 持有 App 借用时回调面板会 `RefCell already borrowed` → abort。
  **绝不在 `&mut App` 回调里直接调 `ShellExecuteW`**。
- 打开外部目标会让对方抢前台，面板因此收起，这是预期，不要把它抢回来。

## 10. 错误处理与异步

- 外部调用失败只写 stderr，不弹窗、不 panic。
- 窗口句柄失效返回 `false`。
- UI 线程不跑阻塞网络/大量磁盘；过滤同步做，命令量很小。
- 不给简单功能引入后台服务或常驻进程。

## 11. GPUI 实测坑（改按键/拖动/跨实体前先读）

- **`Window::start_window_move` 在 gpui-pre 0.3.9 + Windows 是空实现**；拖动只能
  `window_control_area(Drag)` 让系统返回 `HTCAPTION`。
- 拖动区命中测试查的是上一次已分发的鼠标位置；脚本注入要先补一次 move。
- 拖动区里要接鼠标的子控件必须 `.occlude()`（BlockMouse），否则父级拖动区跟着命中。
- List 用 `searchable(false)`，但仍可用 `ListState::set_query`（需 `&mut Window`，走
  `with_launcher` 延后）。
- `Input` 根节点 `size_full()`，宽高由外层控制；`input_px` / `input_py` 看 `Size`，
  行高 1.25rem，容器高减内边距必须 >= 行高。
- `h_flex()` = `flex().flex_row().items_center()`，**交叉轴默认居中**；顶对齐要显式
  `.items_stretch()` / `.items_start()`。`v_flex()` 不设交叉轴。
- **`Entity::update_in` / `WeakEntity::update_in` 只对窗口根视图生效**；只有 `&mut App`
  的回调要用 `with_launcher`。
- 实体 A 的 update 里 update 实体 B 会 panic（`cannot update X while ...`）；跨实体通知
  必须 `app.defer(...)`。
- `ElementId` 没有 `From<(&str,&str)>`，动态 id 用 `SharedString`。
- `RenderOnce` 类型要 `#[derive(IntoElement)]` 才是 `IntoElement`；枚举要手写
  `impl IntoElement`。
- **图标资源**：程序用 `gpui_kit::assets::AllAssets`（全 Lucide 全集），
  `component::IconName` 只是子集；图标名不在子集里时用 `gpui_kit::assets::IconName`。
- **GPUI 全局观察者会自触发**：`global_mut::<G>()` 每次都推通知，`observe_global` 里写
  同一个全局要先去重，否则死循环。

## 12. 开发要求

动手前先看现有结构、依赖版本和相邻模块，不要凭空假设 API。

- 保持现有架构一致，不为当前用不到的东西造抽象。
- 只引入完成需求必需的依赖。
- 改动要分得清：行为变更与纯格式/挪文件分开提交。
- 版本号按改动程度升（功能 = minor，修复/重构 = patch）。

验证口径：

```sh
cargo fmt --check
cargo check          # 零代码警告
cargo test           # 见下
cargo build          # debug
```

- **不做 release 构建**；要看效果用 `cargo run`（debug，保留控制台）。
- 验证前先杀掉正在跑的 `gpui-tools` 进程，再 `cargo run`，免得旧实例占着单例/热键。
- `cargo test` 里 `ui::colors::tests::text_stays_readable_on_a_white/black_wallpaper` 是
  正在调的对比度实验，可能失败，别当回归；`the_slider_never_thins...` 与
  `a_veil_lifts...` 必须过。
- 若一次中断的构建让 `cargo build` 报 `STATUS_ACCESS_VIOLATION`，是增量缓存坏了：
  `CARGO_INCREMENTAL=0 cargo build` 跑一次，或 `cargo clean -p gpui-tools` 后重来。
