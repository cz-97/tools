# AGENTS.md

Rust + GPUI 的 Windows 启动器，个人自用。这份文档是唯一的项目规范：先读它，再动手。

## 1. 项目目标

一个快捷键唤出的搜索面板：输入关键词找命令、算数、转金额、开网址/路径。

核心交互：

1. `Ctrl+Space` 唤出面板（可在设置里改或解绑，托盘兜底）。
2. 输入关键词过滤命令；命中「指令匹配」时结果排在最前面。
3. `↑` / `↓` 选择，`Enter` 执行（复制结果或打开目标）；切到方格布局后是 `↑↓←→` 四向挪。
4. `Esc` 隐藏；在设置/计算器页里 `Esc` 先退回命令页。
5. 面板失去前台（点了别的窗口）自动收起。
6. 拖动搜索栏空白处移动面板（记住纵坐标）。
7. 拖动面板四边四角改大小（记住宽高；有下限，见 §4.8）。
8. 搜索栏右侧齿轮打开面板内设置。
9. **右键点面板顶部的可拖动空白处**切换「钉住」：钉住之后失焦不收起、并且压在其他
   应用上面（见 §4.11）。
10. 托盘：**左键单击唤出面板，右键出菜单**（显示 / 设置 / 重启 / 退出）。
11. 设置里可以自己加「文件」「文件夹」「快捷终端」三类条目：能改、能删、能绑快捷键、
    能决定上不上主页；文件条目还能逐条选「用默认应用打开」还是「打开所在位置」
    （见 §4.9）。
12. 设置里可以开「以管理员身份启动」：靠一条最高权限的计划任务启动，此后不再弹 UAC
    （见 §4.10）。

不追求通用启动器框架，不做插件体系。内置命令是编译进主程序的静态表；用户自己加的那三类
是数据库里的几条记录。

## 2. 技术原则

- Rust + GPUI，不引入 Electron / WebView。
- 依赖要轻、职责要单一。
- 不为抽象而抽象：不提前造扩展体系、SDK、清单格式或进程协议。
- 阻塞操作不得放在 UI 线程。

## 3. 源码结构

工作区三个成员，依赖只能往下走，跨层的调用在编译期就被挡住：

| 成员 | 包名 | 里面是什么 | 允许依赖 |
| --- | --- | --- | --- |
| `crates/platform` | `tools-platform` | Win32 原语：窗口外壳、单例、开机启动、管理员模式、图标、HTTP、对话框、交给 shell 打开 | 无 |
| `crates/core` | `tools-core` | 业务逻辑：计算、拼音、指令匹配、设置持久化、火狐书库、GitHub 查询 | `tools-platform` |
| `crates/app` | `tools` | 界面、命令表、热键、二进制（`tools.exe`） | 上面两个 |

- 版本号只写在根 `Cargo.toml` 的 `[workspace.package]`，三个包 `version.workspace = true`
  继承；依赖版本同理集中在 `[workspace.dependencies]`。升版本改根目录一处。
- 分层是硬约束：`platform` 里不许出现「设置」「命令」「页面」，也不许 `use tools_core`；
  `core` 里不许出现 `gpui`。写到一半发现要反过来依赖，就是分错了，别加 `#[allow]` 绕。
- 各 crate 的 `lib.rs` / `main.rs` 仍用 `pub use` 把子模块重新导出到本 crate 顶层，所以
  自己 crate 内继续写 `crate::settings`、`crate::catalogue` 这类短路径。跨 crate 用全名：
  `tools_core::settings`、`tools_platform::shell`。**别在 app 里再造一层转发模块**。

```text
crates/
  platform/                     tools-platform
    src/
      lib.rs                    mod 声明
      admin.rs                管理员模式（计划任务 / 提权判断 / 日志）
      autostart.rs            HKCU Run 开机启动
      dialog.rs               系统的「打开文件 / 文件夹」对话框
      http.rs                 WinInet 的 GET，GitHub 查询走它
      instance.rs             单例（命名互斥量 + 唤醒事件）
      logo.rs                 纯计算图标光栅化（本地构建脚本也包含它）
      shell.rs                ShellExecuteW 打开外部目标、开新终端
      vsync.rs                DWM 刷新节奏
      window.rs               无边框窗口外壳
  core/                         tools-core
    src/
      lib.rs                    mod 声明 + 顶层 re-export
      logic/                    纯逻辑，可单测
        calculator.rs           表达式求值
        cache.rs                网络结果的文件缓存
        firefox.rs              读火狐系浏览器的 places.sqlite（书签 / 历史 / 图标）
        github.rs               GitHub 仓库查询（个人项目 / 星标 / 搜索）
        matcher.rs              指令匹配（算式 / 金额 / 网址 / 路径）
        pinyin.rs               拼音搜索（原文 / 全拼 / 首字母，中英混排）
        settings.rs             SQLite 持久化
  app/                          tools（bin）
    build.rs                    图标与版本资源
    src/
      main.rs                   bin 入口：mod 声明 + 顶层 re-export
      strings.rs                所有界面文案（改文案只动这里）
      app/                      应用外壳
        mod.rs                  启动流程、全局状态、面板显隐、按键拦截、with_launcher
        actions.rs              命令动作的具体实现
        tray_theme.rs           托盘图标与菜单配色
      command/                  命令与热键
        catalogue.rs            命令静态表 COMMANDS、Section
        shortcuts.rs            全局热键注册 / 冲突检测
      ui/                       界面
        colors.rs               配色表 DARK / LIGHT、面板浓度下限（DARK_FLOOR）
        metrics.rs              圆角四档（改观感先看这里）
        commands_list.rs        主页列表（命令 + 用户工具 + 指令匹配）及其 ListDelegate
        launcher/
          mod.rs                面板状态与行为：导航、换页、动作
          view.rs               绘制：页头、页脚、整页骨架、impl Render
        pages/
          commands.rs           设置里的「命令」页
          settings.rs           设置页各标签
          calculator.rs         计算器一级页面
          lists.rs              工具列表页（书签 / 历史 / 个人项目 / 星标 / 查询仓库）
```

下面各节写 `logic/xxx.rs`、`platform/xxx.rs`、`ui/xxx.rs` 时，路径都是相对**所属 crate
的 `src/`**——文件名没变，只是外面多包了一层 member。

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
- **换页统一走 `Launcher::show_page`**，它对 `Page` 的每个变体都有专门一臂（穷尽 match）。
  新增 `Page` 变体时编译器会逼你补上——以前这段带 `_` 兜底，`Page::UserTools` 就这么被
  兜去了命令页，表现是「收起面板再唤出，回不到自定义条目」。**不要给它加 `_` 兜底。**

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
火狐系浏览器 / GitHub / 文件 / 文件夹 / 快捷终端），左右两栏布局照命令页：左列子标签、
右列内容。加工具 = 给 `SettingsTab::Tools` 下的 `ToolTab` 加一项。

工具页里的文本框（火狐 profile 目录、GitHub 用户名和令牌）**失焦或回车时保存**
（`Launcher::commit_settings_inputs`），不设「保存」按钮；换标签、回主页也会先存一次。

**「指令匹配」标签**：四个匹配各一行开关，见 4.4。

「工具」里的后两个子标签管的是用户自己加的条目，见 4.9。

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

| `ListKind` | 入口命令 | 数据 | 右侧元信息 |
| --- | --- | --- | --- |
| `Bookmarks` | 书签 | `firefox::bookmarks`，每次进来重读 | 页面标题（和书签名不同时） |
| `History` | 历史 | `firefox::history`（最多 500 条） | 访问次数 · 最后访问 |
| `Projects` | 个人项目 | `github::owned_repos`，**只读缓存** | 主语言 |
| `Stars` | 星标 | `github::starred`，**只读缓存** | 主语言 |
| `Search` | 查询仓库 | `github::search`，**回车 / 按钮才搜** | 主语言 |

**行永远是两行**：第一行名字（右边挂元信息，`text_ellipsis`），第二行描述（书签和历史
是网址，仓库是「★ 星数 · 简介」）。不要把名字和描述塞回一行——描述独占一行才不跟名字
抢宽度。

页头从左到右：这一页的图标、标题、搜索框（空着时定宽，见 §5.1）、
`flex_1`、**右上角那枚按钮**。按钮只在需要手动去取的页面出现（`ListKind::needs_action`）：
仓库两页是「重新去网上取」，查询仓库是「再搜一次」；**书签和历史每次进页面重读库，
天生最新，不给按钮**。返回只走 `Esc`（页脚有提示），左上角不挂返回箭头。

- **搜索现场会留一份**：面板收起时如果停在查询仓库页，`remember_page` 把关键词、结果、
  高亮位置存进 `Launcher::search_state`；`resume_seconds` 内再唤出直接摆回去（不重搜）。
  场景就是「搜到 → 双击打开 → 面板被浏览器顶掉 → 再唤出接着挑」。
  回主页（`back_to_commands`）或过了保持期就清掉，不跨会话攒。
- **联网时机（用户明确要求）**：只有「按刷新」和「按回车/搜索按钮」才发请求。
  进页面**一个包都不发**——仓库两页没缓存就显示「点右上角刷新一次」干等着，绝不自动抓。
- **搜索**：本地页（前四个）输入即过滤，不过滤也可以看；`Search` 页不做本地过滤
  （`language:rust` 这类限定符会被滤没），打字时不搜，回车或点右上角按钮才发请求；
  同一个词再回车就是「打开选中的仓库」。
- **拼音搜索**：过滤一律走 `logic/pinyin.rs` 的 `pinyin::matches(haystack, query)`，
  主页（命令 + 自定义条目）、五个列表页、设置里的自定义条目页三处都是它。规则是
  「搜索词按空白切段，每段都要命中；每段先按原文（不区分大小写）找，找不到再看拼音」：
  全拼（`dakaishezhi`）、首拼（`dksz`）、中英混排（`githubxb` / `gxb` /
  `githubxingbiao`）都认。**不要**在别处再写一遍 `to_lowercase().contains()`，
  那就是把拼音支持漏掉一条入口。
- **焦点**：列表页的默认焦点永远是搜索框；点完行、点完右上角按钮都要把焦点还回去
  （`focus_list_search`）。**不要**做 `Tab` 切焦点的机制——试过，用户嫌复杂。
- **`tab` 必须在拦截器里吞掉**：gpui-base 的 `Root` 把 `tab` 绑成了切焦点，焦点一旦
  跑去按钮上，敲字母数字就完全没反应（`app::intercept_key` 里 `tab` 分支 +
  `Launcher::focus_active_input`）。另外对可打印字符有兜底：焦点不在任何输入框时，
  **这一下字符要直接补进本页搜索框**（`intercept_key` → `type_into_search` →
  `Launcher::type_into_search`），**不许只聚焦再吞掉**——那样用户点一下列表再打 `ip`，
  只有 `p` 进得去（报过两次）。主页、书库几页、自定义条目三处共用这一条，别再写第二份。
  设置页 / 计算器页没有「默认输入框」，那里仍走 `focus_active_input`。
- **点击**：行自己挂 `on_click`（`ListState` 建的时候 `selectable(false)`，不然
  `List` 的 `confirm` 分不出单击双击）。**单击** = 选中 + 复制链接，**双击** = 打开。
- 行是 `Entry`：标题 / 描述 / 右侧元信息 / 打开目标 / 站点图标。目标一律交给
  `shell::open`，**不要**在 UI 回调里直接 `ShellExecuteW`。
- 缓存：`%APPDATA%\tools\cache\github-{projects|stars}-{user}.json`。进页面只读它，
  有就摆出来、没有就留一句提示；要新的只有右上角那枚按钮。
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

命令页（设置 → 命令）左右两栏：左列**目录里真的有命令的子模块**（`catalogue::sections()`，
从 `COMMANDS` 现算，没命令的分组不出现），右列该分组的命令行。
每行：图标 + 名称/描述 + 调用次数 + 快捷键按钮 + 启用开关；左列每项显示「启用数/总数」。

用户自己加的工具（文件快捷方式 / 快捷终端）不进这张表，它们在「设置 → 工具」里管，
但**在主页上和命令同池排序**（见 4.9）。

### 4.7 快捷键

- 左键录制 / 右键清除（`command/shortcuts.rs` + `ui/pages/commands.rs` 的
  `shortcut_button`，命令页与系统页共用）。
- 录制期间 `intercept_keystrokes` 吞掉所有按键；`Esc` 取消、`Backspace`/`Delete` 清除、
  无修饰键忽略。
- **冲突不靠拦截判**：`RegisterHotKey` 会把组合键从焦点窗口截走，冲突从热键事件那条路
  报回（`report_conflict`）。**面板隐藏必须结束录制**（`leave`），否则所有热键静默失效。
- 唤出热键和命令热键同源：`settings.summon_shortcut`；`none` 表示明确不绑（托盘兜底）。
- 用户工具的快捷键也走这一套（`ShortcutOwner::Tool(id)`）；`apply_shortcuts` 一次把
  静态命令和工具全部注销再全量注册，删掉一条工具时也要调它（那个键得放出来）。
- 键名用 `keyboard-types` 的 `Code` 存，显示时才友好化。

### 4.8 面板外壳、尺寸与空闲开销

- 窗口是 `WindowKind::PopUp` + `titlebar: None`，边框由宿主自己摘
  （`NativeWindow::apply_panel_chrome`）。**`WS_THICKFRAME` 必须留着**：系统只认它决定
  窗口边缘能不能拖。它的那圈非客户区由 `window.rs` 里挂的子类过程吃掉（`WM_NCCALCSIZE`
  直接返回 0，客户区铺满整扇窗），否则四周会露出一圈透明缝。
- **四条边的命中区自己算**：GPUI 只在**上边**判断拉伸（`events.rs` 的 `handle_hit_test_msg`
  只比较 `cursor_point.y <= frame_y`），左右和下边落到 `DefWindowProc` 只剩一像素。
  子类过程里 `resize_hit_test` 按 `SM_CXSIZEFRAME + SM_CXPADDEDBORDER` 算一圈，返回
  `HTLEFT/HTRIGHT/...`；不在边缘就交回 GPUI（页头的拖动区在那里）。
- 尺寸存在 `Settings::window_width` / `window_height`（**逻辑像素**，换 DPI 打开还是同样大），
  只在 `remember_window_position`（收起面板 / 退出）时回写；下限
  `WINDOW_WIDTH_MIN` / `WINDOW_HEIGHT_MIN` 同时喂给 `WindowOptions::window_min_size`。
- **视口要手动对账**：GPUI 的 `viewport_size` 只在它自己的平台回调里更新，而窗口是在
  `run` 的回调里摆好的（消息循环还没开始），不等一下就会一直按创建时的显示器尺寸排版，
  界面右下角被裁掉。`app::settle_viewport` 在循环跑起来之后调一次
  `Window::bounds_changed` 把视口拉回真实客户区。
- 正文高度按 `viewport - 页头 - 页脚` 算死（`launcher/view.rs` 的 `FOOTER_HEIGHT`），
  不要指望 flex 收缩：`List` 内部是 `size_full()`，外层拿不到确定高度就按内容高度排版，
  多出来的部分会盖住页脚。
- **空闲时不许轮询**。全局热键（`global_hotkey` 的 channel）和「别的进程叫我们亮面板」
  （命名事件）都走一条阻塞的后台线程 + `futures` channel 交给前台，删掉 80ms / 200ms
  的 `try_recv` 循环。
- 剩下那点固定开销来自 GPUI 的 VSync 线程：`gpui-pre-windows` 的 `begin_vsync_thread`
  无条件每 16.6ms 醒一次并对所有窗口 `RedrawWindow`，与可见性无关。GPUI 没有开关，
  所以 `platform::vsync` 按线程名（`VSyncProvider`）找到它，**面板收起 3 秒后挂起、
  唤出前恢复**（`hide_launcher` → `schedule_render_suspend`，`show_launcher` 第一件事
  就是 `resume`）。挂起期间主线程不渲染，恢复一定早于显示。实测隐藏态从 4~6% 降到
  **0.000%**（单核，debug 构建）。

### 4.9 用户工具（文件 / 文件夹 / 快捷终端）

静态命令写在 `COMMANDS` 里，编译期就定了；这三类是**用户在设置页里自己加的**，存在
`tools` 表里（`Settings::tools`），能增、能删、能改：

| 种类 | `target` 存什么 | 执行时做什么 |
| --- | --- | --- |
| `ToolKind::File` | 文件路径 | 默认 `shell::open_location`（打开它所在的文件夹）；条目上那枚胶囊切到「默认应用」后改走 `shell::open` |
| `ToolKind::Folder` | 文件夹路径 | `shell::open_location`：是目录就打开它自己 |
| `ToolKind::Terminal` | 一整条命令行 | 默认 `shell::run_in_terminal`：**优先 Windows Terminal**（`wt -d <目录> <pwsh> -NoLogo -NoExit -Command <命令>`），没装 wt 才退回 `cmd /K` + 新控制台。条目上那枚胶囊切到「静默跑」后改走 `shell::run_quiet`（能直接起就 `CreateProcess`，否则 `cmd /C`，都不开窗口）。工作目录是 `start_dir`，留空 = `%USERPROFILE%` |

- **数据和静态命令一样「一条管自己」**：`UserTool` 上挂着名字、`target`、开关、次数、
  快捷键，外加两个只对某些种类有意义的字段——`open_default`（文件：默认应用还是位置）、
  `start_dir`（终端的工作目录）。不去挤 `commands` 表的键空间（那张表按「子模块 + id」
  定位静态命令）。id 只增不回收。
- **文件条目的「默认应用 / 打开位置」是每一条自己的**，不是全局开关：行尾那枚胶囊
  （`mode_chip`）点一下切。理由很实际——有的文件想直接开，有的只想看看它在哪。
- **「静默跑」不必经过 cmd**：第一个词能解析成真的 `.exe`/`.com`、且整条命令里没有
  shell 语法时，直接 `CreateProcess` 那个程序 + `CREATE_NO_WINDOW`（`rundll32 <cpl>`、
  `ipconfig`、`shutdown /r` 都在这一类）；只有内建命令和管道/重定向/`%变量%` 才上
  `cmd /C`——那些语法除了 cmd 没人认（`needs_shell`）。
  **这里不能学 Win+R 用 `ShellExecute`**：Win+R 确实是把整串丢给 shell 解析，但它给
  控制台程序开的窗口是**可见**的，静默要的正是连黑框都不闪。
- **提权下开终端要绕一下**：`wt.exe` 是「应用执行别名」，提权进程里直接 `CreateProcess`
  它，参数会在激活那一跳被吃掉（Windows 的 bug）。提权时改走 `ShellExecuteW` 的 `runas`
  动词——那条激活路径参数不丢，而且调用者已提权，不会再问 UAC。
  **别用 `powershell -Command "Start-Process -Verb RunAs ..."` 当中转**：powershell 是
  控制台程序，从提权进程 spawn 会先闪一个黑框，正是用户要去掉的那个闪烁。
  判断条件用 `admin::is_elevated()`（当前进程提权没有），不是设置里的开关。
- **主页上与静态命令同池排序**：`ToolDelegate` 把命令和工具收进同一张 `Vec<HomeItem>`
  按次数排（`rebuild` 里排序，结果缓存在 `rows`——一次渲染里 `item_count` /
  `item_at` / `render_item` 都会问它，现算就是每轮重排一次）。
  搜索词同时匹配名字和 `target`（路径或命令），两个都该能搜到。
- 快捷键：`ShortcutOwner::Tool(id)` / `RecordTarget::Tool(id)`，和命令共用一套录制、
  冲突检测和 `apply_shortcuts` 全量重绑。
- 新增文件 / 文件夹**先弹系统对话框**（`platform/dialog.rs`：文件是
  `GetOpenFileNameW`，目录是 `IFileOpenDialog` + `FOS_PICKFOLDERS`；都是阻塞接口、
  跑在后台线程）。**两者必须是同一个对话框**：目录框以前用 `SHBrowseForFolderW`，
  那个老树形窗口用户一眼就看出来「不是文件选择器那种对话框」。
  `windows-sys` 没有 COM 接口，选目录这条路依赖 `windows` crate（`Cargo.toml` 里只开了
  Foundation / Com / Shell 三个 feature，别全开）。
  选完按文件（夹）名建一条；名字想改就点行尾那支笔。
- **挑什么框由「种类」决定，别写死**：`PickFor::EditorTarget` 要看编辑区当前的
  `kind`（文件夹条目 → 目录框），`PickFor::Replace(id)` 看数据库里那一条的 kind。
  之前编辑区里写死 `false`（永远文件框），文件夹条目点「选择文件夹」弹出的是文件框，
  在框里双击目录只会进去、选不中（用户报过）。
- 对话框开着时 `Launcher::dialog` 是 `Some`，`watch_focus` 据此**不收起面板**——用户
  正在挑文件，面板一消失就白挑。这个字段**不要和 `pending` 合并**：`pending` 会跟着
  换页被丢掉，丢了这个任务就等于对话框永远开着，自动收起从此失效。关掉对话框之后还有
  `front_grace`（1.2 秒）宽限：前台转回来要一两百毫秒，不拦一下面板会自己消失。
- 执行时路径不在了（文件和文件夹都会）：不计数，弹 `Launcher::prompt`（盖在整页上的
  那张卡片），三个出口——`修改`（按种类再弹一次对话框、换掉 `target`）、`删除`（连带
  放掉它的快捷键）、`取消`。
- 编辑区（`Launcher::tool_editor`）是就地插在列表上方的一块表单：名字 + 目标，终端再加
  一行起始目录。**名字或目标空着不保存**；回车等于保存，换标签 / 回主页等于放弃。
  - **名字那一栏可以留空**：文件和文件夹用路径最后一段兜底（`name_from_path`，
    `D:\资料\周报.docx` → `周报.docx`）。只有终端的命令（和盘符根目录）推不出名字，
    仍然报「名字不能空」。
  - **聚焦必须等这一帧画完**：编辑区是这一拍才塞进元素树的，元素在树里之前
    `window.focus` 一律落空。`window.defer` **不管用**——它在当前 effect cycle 末尾
    就跑，仍在渲染之前。要用 `window.on_next_frame`（文档原文：directly after the
    current frame is rendered）。少了这一步，用户点开「改」立刻用中文输入法打字，
    第一个字母进不了输入法上下文，只会变成一个孤立的字母（用户报过两次）。
  - **点完行内控件（快捷键 / 开关 / 模式 / 删除）要把焦点还给搜索框**：焦点停在
    按钮上，用户接着打的第一个键同样进不了输入法。`cancel_tool_editor` 和
    `save_tool_editor` 以前还 `window.blur`，焦点直接悬空，现已改成
    `focus_active_input`。
- **页头那排控件（`ui/launcher/view.rs` 的 `user_tools_header`）**：类型分段（`segmented_compact`）
  → 开启状态筛选 → 新增。三者的高度都吃 `settings_page::HEADER_CONTROL_HEIGHT`，
  两个按钮用 `small_button_fixed` 定宽（`HEADER_BUTTON_WIDTH`）——文字长短不一样时
  按钮不会左右跳。**别在这里另起一套尺寸**，页头歪一两个像素很明显。
- **开启状态筛选**（`Launcher::user_filter`，`EnabledFilter`）是三态循环按钮：
  全部 → 已开启 → 未开启 → 全部，判定在 `EnabledFilter::keeps`，过滤发生在
  `UserToolsDelegate::retarget`（重建 `rows` 缓存）里。改筛选或换类型都要把高亮打回第一行
  （行数会变）。
- **条目是缓存的，不是现算的**：`items_count` / `render_item` 每帧每行都要问一遍，
  在那里现读设置再全量克隆就是「滚动卡顿」。`UserToolsDelegate::rows` 只在 `retarget`
  里重建，而 `retarget` 由 `Launcher::sync_user_list` 调。
- **所有「改数据」的动作都必须让缓存失效，统一挂在 `refresh_catalogue` 上**：增删改、
  开关、静默/默认应用、绑快捷键都是在改完之后调它，所以 `refresh_catalogue` 结尾
  一并 `sync_user_list`。**别再往单个动作里补**——补一处漏三处，之前就是只在几个动作里
  手动调，结果开关点了没反应、新增的条目要退出再进才出现（用户报过）。新写一个改
  设置的动作时，只要记得「改完调 `refresh_catalogue`」这一条就够。
- **行内控件的 element id 用数据库主键（`tool.id`），不要用行号**：行号会随筛选、增删、
  排序换位置，而 gpui 的命中链和 `Switch` 之类的动画状态都是按 id 记的。用行号时列表
  一重建就把第 3 行的状态套到新的第 3 行上——用户看到的是「点一下快捷键，后面几行的
  开关自己动」。
- **选中态归 `ListState`，delegate 里那个只是镜像**：行上画的选中来自
  `ListState::set_selected_index`，只改 delegate 的 `selected` 字段画面上一点反应都没有
  （用户报过「↑↓ 没反应」）。所以上下键和单击都要 `set_selected_index` +
  `scroll_to_selected_item`，和 `move_entry` / `entry_clicked` 一个写法。
  `UserToolRow` 的 `Selectable::selected` 故意**不认**框架推下来的值：增删条目之后
  `ListState` 里那个下标会过期，认了就是「高亮在 A、回车执行 B」。
- **回车也要在这里办**：焦点在搜索框里，`List` 的 `confirm` 派不下来，
  `activate_selected` 的 `Page::UserTools` 分支自己取 `selected_id()` 执行。
- **行里的控件用 `block_mouse_except_scroll`，不要用 `occlude`**：`occlude` 是
  `HitboxBehavior::BlockMouse`，会把鼠标底下的 hitbox 从命中链里截断，列表的滚动容器
  就再也拿不到 `should_handle_scroll`——表现是「鼠标停在按钮/开关上滚不动列表」。
  前者只挡点击和悬停，滚轮照传（见 §11）。
- **这一页的现场和仓库搜索页一个规矩**：收起面板时 `remember_page` 把类别、筛选、
  关键词、高亮存进 `Launcher::user_state`，`resume_seconds` 内再唤出由 `open_user_tools`
  摆回去；从主页命令进来则是干净的一页（类别回 `File`、筛选回 `All`、关键词清空）。
  判定方式照 `open_list`：当前还是这一页 = 从记忆里回来的。

### 4.10 托盘、管理员模式与开机启动
- **托盘左键单击唤出面板，右键出菜单**。`gpui-tray` 的 Windows 后端在「有菜单、没配
  激活回调」时会把左键也变成弹菜单，所以 `build_tray` 必须挂 `.on_activate(ShowLauncher)`。
- **管理员模式 = 一条计划任务**（`platform/admin.rs`，任务名固定 `tools-launcher`）。
  系统里有了这条任务，`schtasks /Run` 拉起来的进程**天生提权、不弹 UAC**——这就是
  「启动无需授权弹窗」的全部秘密。建任务那一下本身要提权（`runas` 动词拉一个提权的
  schtasks），所以**开启时会问一次**，之后（登录启动、手动启动、重启）都不再问。
- **任务用 XML 建，不用 `schtasks /Create` 那堆开关**：`/Create` 没有「电池供电也运行」
  的开关，而它建出来的任务默认 `DisallowStartIfOnBatteries=true`——**笔记本一拔电源就
  排队不执行**，表现正是「退出后再也起不来」（踩过）。斜杠参数也没有执行时限开关，默认
  `PT72H` 会把常驻的启动器在三天后掐掉，XML 里给 `PT0S`。XML 没建成才退回 `/Create`。
- 任务在不在是**事实来源**：启动时 `settings.run_as_admin = admin::installed()`，和
  `launch_at_login = autostart::enabled() || admin::installed()` 一个路子。但**开关开着
  不等于眼前这个进程是管理员**——只有用任务拉起来的那次才是，所以还有
  `admin::is_elevated()`，系统页据此显示「重启一次才会生效」并给一枚重启按钮。
- **`install` 成不成功看任务在不在（`query` 带重试），不看 schtasks 退出码**：`/Create`
  有时会因为无关紧要的东西给非 0，拿退出码当结论就会「建好了却当没建成」。
- **`Launcher::admin_busy` 才是开关的锁，不要用 `admin_task.is_some()`**：Task 跑完句柄
  还留在字段里，拿它当守卫会让开关永久卡死（关掉之后再也点不开，只有重启才恢复）。
- 关掉管理员模式**不会**让当前进程降权（子进程也降不了）：要等下次正常启动。所以
  「关掉后点重启」仍然是提权进程，这是 Windows 的机制，不是 bug。
- 这套东西跨进程跨权限，正常启动又没有控制台（`eprintln!` 是写进虚空的），所以
  `admin::log` 往 `%APPDATA%\tools\admin.log` 落几行关键事实（启动时提权没有、schtasks
  每次调用及结果）。排查先把那个文件打开。
- 管理员模式下开机启动由任务的 ONLOGON 触发器负责，注册表那条 Run 会被删掉，避免两个
  实例抢单例；把「开机启动」关掉等于把任务也删掉（两者是同一件事）。
- **重启必须走任务**（`actions::restart_launcher`）：直接 spawn 出来的是普通权限进程，
  提权就白开了。
- **重启分两条，按 `is_elevated()` 判**：已经提权就直接 `spawn_self`（子进程继承提权
  令牌，无 UAC、无队列、无延迟）；没提权才让任务去启动。
- **托盘可能建不出来，要重试**：登录时被任务拉起的那一次，Explorer 还没准备好通知区域，
  `Shell_NotifyIcon` 会直接失败（`gpui-tray` 的 `build` 返回 `Err`）。`build_tray` 失败后
  每秒重试一次、最多 30 次，成败都写进 `admin.log`。别指望 `TaskbarCreated` 广播兜底——
  窗口压根没建起来时没人接那个广播。
- **手动启动也要补提权**（`app::elevate_handoff`，在抢单例**之前**）：管理员模式开着、
  但这次是双击 exe 起来的（没提权），就把启动交给任务、等新实例出现再退。不做这件事
  的话手动启动永远是普通权限实例，系统页那颗「重启」按钮会一直亮着。用
  `admin::take_hop_slot()`（8 秒冷却）防止任务给的是普通权限时两个进程来回蹦。
- **交给任务之后必须确认新进程真的起来了**（`instance::wait_for_new`，3 秒）再退出：
  `schtasks /Run` 返回成功只代表任务被接受，任务可能一直排队——拿它当证据就是
  「退出后什么都没有」。等不到就 `spawn_self` 兜底，宁可给个普通权限的实例。
- **要「结束」这个进程，用 `tools.exe --quit`，不要 `taskkill`**：提权的进程，普通权限
  的 `taskkill` 一律拒绝访问，`Stop-Process` 同理，只能靠 `Start-Process -Verb RunAs`
  弹一次 UAC。`--quit` 走的是单例那套命名事件（`Local\tools.quit`）：新进程一个副作用
  都不做（不提权、不抢单例），只给事件打个信号就退，跑着的那个收到信号自己
  `remember_window_position` + `cx.quit()`。**这条分支必须在 `elevate_handoff` 之前**，
  否则 `--quit` 会先把一个提权实例拉起来再退。

### 4.11 钉住面板（右键切换）

右键点面板顶部的可拖动空白处切换「钉住」。**钉住 = 两件事**：失焦不收起 + 窗口压在
其他应用上面。没有设置项，这是右键的专属动作。

- **必须做在原生窗口过程里**（`platform/window.rs` 的 `panel_proc`）：拖动区的命中码是
  `HTCAPTION`，对系统来说那就是标题栏，右键走的是 `WM_NCRBUTTONUP`，**根本到不了 GPUI
  的元素树**——应用层的 `on_mouse_down(Right)` 收不到。那里只处理 `wparam == HTCAPTION`
  的分支，切完返回 0 拦掉系统菜单。
- 「压在最前」= 窗口的 `WS_EX_TOPMOST`（`SetWindowPos` + `HWND_TOPMOST/NOTOPMOST`），
  `SWP_NOACTIVATE` 别丢：切换不该把焦点抢走。
- 「失焦不收起」在 `Launcher::watch_focus` 里：那个循环本来就在采前台，顺带读一次
  `window.is_topmost()`，钉住就不累计丢焦次数。**不要另存一份布尔值当真相**——原生那条
  路改的就是窗口属性，读窗口永远是最新值，应用那边的镜像（`Settings::pinned`）只是给
  下次启动恢复用的。
- `store_settings` 保存前用 `window.is_topmost()` 对一次账，界面判断也问窗口。不做这一步，
  下次保存会用内存里的旧值把用户刚切的状态洗掉。
- 启动时按 `settings.pinned` 恢复（窗口属性不会自己跨进程留住）。
- `Esc` 照样收面板：钉住只是挡住「因为失焦而收起」，挡不住用户明说要收。

### 4.12 主页方格布局

外观设置里可以把主页在「列表 / 方格」之间切。方格只显示图标和名字，鼠标压上去才出描述。

- **不复用 `List`**：它要求「Every item should have same height」，而命中行（40px 左右）
  和方格行（边长 72–112）天然不同高，混进一个 List 只会把命中行撑成方格高度。所以方格
  这一版是自绘的：`ui/launcher/view.rs` 的 `home_grid` 直接排 `div`，滚动用
  `overflow_y_scroll` + `ScrollHandle`，高亮、命中、滚动全自己算。
- **边长算法**在 `ui/grid.rs`（有单测）。给定内容宽度 W、间距 g、下限 m、上限 M：
  - 不超上限的最少列数 `ceil((W+g)/(M+g))`
  - 不小于下限的最多列数 `floor((W+g)/(m+g))`
  - 取两者较小值，再用可用宽度把边长铺满，夹在 `[m, M]`。
  结果就是「先让方格变大，够到上限再加一列、边长回到下限」；加列的门槛是
  `columns*m + (columns-1)*g`，这是唯一能同时守住上下限的算法——先定列数再截断边长会
  在右侧永远留白。
- **命中（指令匹配）仍然是行**，钉在网格上方、不参与滚动；`↑↓` 在命中区里是上一条
  下一条，越过最后一条落到第一个方格，从第一行往上回到命中区。
- 绘制和方向键都调同一个 `grid::metrics_for(视口宽度)`：两边算出来的列数必须一致，否则
  按一下右箭头跳到的位置和屏幕上看到的那格对不上。
- 悬停描述是网格容器里一个绝对定位的小方块（位置由行列算出来），**不是** `Tooltip`
  组件：面板是无边框浮窗，浮层越界会被窗口裁掉，自己算位置才能把右边夹住。

### 4.13 文件 / 终端的系统图标

用户加的文件和快捷终端，图标**不是编译进来的那套**，而是资源管理器显示的那个：

- `ui/icons.rs` 用 `SHGetFileInfoW` + `SHGFI_ICON | SHGFI_LARGEICON` —— 资源管理器画图标
  走的就是这条，所以 `.exe` 是它自己的图标、`.msc` 是任务计划程序那个、没关联的扩展名是
  默认白纸、有默认程序的是那个程序的图标，全自动对上，不需要维护映射表。
- 快捷终端先把命令的第一个词解析成程序路径（`shell::program_of`，`ipconfig` →
  `…\System32\ipconfig.exe`），再取那个程序的图标；解析不出来（`cd`、`dir` 这类内建）
  就退回内置图标。
- `HICON` → RGBA：`GetIconInfo` 拿 `hbmColor`，`GetDIBits` 按 32 位读出来（负 `biHeight`
  省掉行翻转），`BGRA → RGBA` 交换；alpha 全 0 的老图标用 `hbmMask` 补透明度。
- **必须缓存**（`Arc<RenderImage>`，键是「种类 + 目标路径」）：取一次要几毫秒，每帧重取会
  把面板拖垮。`None` 也要缓存——「问过了系统也没有」不该每帧再问一遍。
- 取不到就退回内置 `IconName`，别让格子空着。

## 5. 交互细节

### 5.1 搜索栏与拖动

搜索栏是面板唯一的拖动面，用 `WindowControlArea::Drag` 交给系统（不要自己模拟拖动）。

- 搜索栏容器整体是拖动区；**需要鼠标的控件必须 `.occlude()`**，否则按下会被当成拖标题栏。
- 搜索框宽度跟随文字（`search_field_width`），不用 `flex_1`：多占一像素就少一像素可拖。
- **不画占位提示**（用户明确要求，2026-11-10）：搜索框空着时给固定宽度
  （`SEARCH_FIELD_IDLE` / 窄页头用 `SEARCH_FIELD_IDLE_NARROW`），否则框会缩成一个光标、
  整条页头都点不进去。Input 也不要 `placeholder`。
- 搜索框用 `Size::Small`（`input_py = 2`）；Medium 的 8+8 内边距会裁掉字形。容器高度按
  `settings.search_height` 给足，Input 根节点 `size_full().items_center()` 会自动居中。

### 5.2 键盘

输入框是唯一持有焦点的控件，List 不是焦点视图，所以按键统一在 `App::intercept_keystrokes`
里处理（`enter` / `up` / `down` / `escape`），经 `with_launcher` 回到视图：

- `up/down`：命令页移动高亮，设置页切换标签。
- `enter`：执行高亮项（`activate_selected`，分派匹配或命令）；列表页里打开高亮那一行。
- 列表页的搜索框常驻焦点，`↑↓` 仍由拦截器收走做选中移动，不进输入框。
- `escape`：设置/计算器页先回命令页，否则收面板。
- **计算器行、工具编辑区的输入框有焦点时 `up/down/enter` 一律放行**
  （`app::typing_in_field`），那几个键在框里是光标移动、换行、提交表单。
- `enter` 在设置页里：编辑区开着就是「保存」，否则把改过的文本框落盘。
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
- **系统文件框开着时不收**（`Launcher::dialog` 是 `Some`）：前台在对话框身上，
  那不是用户跑掉了。
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
- 面板浓度的**下限是写死的 `DARK_FLOOR = 0.64`**：`#1f1f1f` 铺到 64% 合成 `#70`，
  白字 4.6:1。低于它就开始「发虚」——0.30 合成 `#a2`（2.5:1）、0 合成 `#da`（1.3:1）。
- **别再回头去「按壁纸亮度现算下限」**（0.37.0 试过，已撤）：壁纸和面板底下真正参与
  合成的那一层不是同一样东西。实测面板在 0% 时是 `#da`，而当时背后是深色窗口——
  说明底下压着一层接近白的底子，量壁纸只会把下限算成 0，直接把字糊掉。
- 浅色主题没有下限（`LIGHT_FLOOR = 0`）：近白面板叠在近白底子上，黑字对比只会更高。
- 滑块整段行程 = `[下限, 1.0]`，所以每一格都还有用（不会出现拖了没反应的死区）。
- 面板越薄，字色越要往白里提（`text_lift`），否则浅灰字压在浅灰底上必糊。
- 想改「多透」就调 `DARK_FLOOR`：改小 = 更透但字更糊，改大 = 更稳但面板更实。
  改完 `cargo test -p tools colors` 会把整段行程逐档验一遍对比度。
- 面板外沿 1px 高光边（`UiColors::panel_edge`）是玻璃感的关键，别删；`EDGE_MIX` **不能是 0**
  （0 等于高光等于面板色，那条线画了看不见）。
- 选中态（`accent`）和悬停（`hover`）的浓度要「一眼认得出来」：12% 的选中块在深色面板上
  读不出光标停在哪，5% 的悬停等于没有。
- **着重色只有中性灰**（用户明确要求，2026-11-10）：`accent` / `accent_foreground` /
  `on_accent_muted` 不许带色相，选中靠明度说话。别再往表里塞蓝、紫之类的强调色。
- **验证亚克力只能看屏幕合成**：BitBlt / PrintWindow 抓不到 DWM 合成内容，靠用户看。

### 6.3 圆角与几何

- 圆角只有四档，都在 `ui/metrics.rs`：`RADIUS_CHIP`(4) 键帽与行内小块、
  `RADIUS_CONTROL`(6) 按钮/分段内块/列表行、`RADIUS_SURFACE`(8) 分段底座与方格、
  `RADIUS_CARD`(10) 浮层卡片。**不要写 `rounded(px(8.))` 这类字面量**，新增一档先想清楚它嵌在谁里面。
- 嵌套遵循「内层 = 外层 − 间隙」：分段底座 8、内块 6、中间 `p_0p5`(2)。同值嵌套在转角处会糊成一条粗边。
- 面板自己的圆角是 `launcher::CORNER_RADIUS`，必须等于系统窗口裁剪（物理像素，不随 DPI），不在上面那套刻度里。
- **共享左边距 16**：页头 `px_4`、页脚 `px_4`、正文行（`px_1` 容器 + `px_3` 行）三者第一个元素
  必须站在同一条竖线上。差 4px 肉眼就看得出「左边被推了一下」。

## 7. 图标

`platform/logo.rs` 是唯一图形来源：纯计算光栅化，`rgba(size)` 给托盘，`ico()` 给 exe。
`crates/app/build.rs` 用 `#[path = "../platform/src/logo.rs"]` 把这一个文件当模块包含进来
（它自己不带依赖，所以构建脚本能直接用），生成 `.ico` + `.rc`，找 `rc.exe` 编成 `.res`
链进去；找不到 `rc.exe` 只警告、照常构建。窗口图标由 `NativeWindow::set_icon(1)` 从 exe
资源 id 1 取。改图形只动 `logo.rs`，构建脚本那边不用跟着改。

## 8. 设置持久化（`logic/settings.rs`）

SQLite：`%APPDATA%\tools\tools.db`，`rusqlite` 的 `bundled`。两张表，整份一次
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
- 网络结果不进这张库：它们存在 `%APPDATA%\tools\cache\<键>.json`（见 `logic/cache.rs`），
  丢了从网上再抓一次就有。
- `commands` 主键 `(section, id)`；只有用户改过的命令才有行。
- 读不出来就退回默认并写 stderr，绝不让程序起不来。
- **不写迁移**：自用项目，改表结构直接删库重建，别留 `PRAGMA user_version` 分支。
- `uses` 在内存累加，收起面板时随整份记录落盘——`remember_window_position` **必须每次
  隐藏都写**，不能「window_y 没变就早退」。

## 9. 外部程序调用

- 调外部程序一律用参数数组（`Command::new(..).arg(..)`），不拼 Shell 命令。
- **打开文件夹 / 链接一律 `tools::shell::open`**（`ShellExecuteW`，verb `open`）：
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
- **但可滚动容器里的子控件不能用 `occlude`，要用 `block_mouse_except_scroll`**：
  `occlude` 是 `HitboxBehavior::BlockMouse`，`hit_test` 一碰到它就把后面的 hitbox 全丢掉
  （`window.rs` 的 `hit_test` 里直接 `break`），滚动容器因此拿不到
  `should_handle_scroll`——「鼠标停在按钮/开关上滚不动」。后者是
  `BlockMouseExceptScroll`，只把 `is_hovered` 置假，滚轮照传。
- **`ListState::set_selected_index` 才是选中态的真相**：`List` 渲染每个 item 时会用
  它自己那个 `selected_index` 覆盖 delegate 传下来的 `selected()`。只改 delegate 字段 =
  画面不动（「↑↓ 没反应」）。滚动也一样，`scroll_to_selected_item` 读的就是它。
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

验证口径（在仓库根目录跑，命令默认作用于整个工作区）：

```sh
cargo fmt --check
cargo check          # 零代码警告
cargo test           # 见下
cargo build          # debug
```

- 只跑一块用 `-p`：`cargo test -p tools-core` 是纯逻辑那一套（快），
  `cargo test -p tools` 是界面那一套；`cargo check -p tools-platform` 只查平台层。
- **不做 release 构建**；要看效果用 `cargo run`（debug，保留控制台）。
- 验证前先杀掉正在跑的 `tools` 进程，再 `cargo run`，免得旧实例占着单例/热键。
- `cargo test` 里 `ui::colors::tests::text_stays_readable_on_a_white/black_wallpaper` 是
  正在调的对比度实验，可能失败，别当回归；`the_slider_never_thins...` 与
  `a_veil_lifts...` 必须过。
- 若一次中断的构建让 `cargo build` 报 `STATUS_ACCESS_VIOLATION`，是增量缓存坏了：
  `CARGO_INCREMENTAL=0 cargo build` 跑一次，或 `cargo clean -p tools` 后重来。
