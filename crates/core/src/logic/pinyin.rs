//! 拼音搜索：把一段文本展开成「原文 / 全拼 / 首字母」几种形态，供各列表过滤用。
//!
//! 匹配顺序是「先原文、后拼音」：搜索词能直接在原文里找到就一分钱不花，
//! 只有原文找不到的那一段才去展开拼音。所以纯中文、纯英文的搜索和从前一样快，
//! 拼音只是多出来的一条兜底路子。
//!
//! 四种形态（以「GitHub 星标」为例）：
//!
//! | 形态 | 内容 | 能搜到 |
//! | --- | --- | --- |
//! | 原文 | `github 星标` | `星标`、`github` |
//! | 全拼（按字隔开） | `github xing biao ` | `xing`、`biao` |
//! | 全拼（连写） | `githubxingbiao` | `xingbiao`、`githubxing` |
//! | 首字母（英文留整词） | `githubxb` | `xb`、`githubxb` |
//! | 首字母（英文也缩写） | `gxb` | `gxb` |
//!
//! 中英混杂不需要特殊处理：字母数字原样留着，汉字换成拼音，两边自然接在同一串上。

use pinyin::ToPinyin;

/// 一段文本展开后的拼音形态。
pub struct Folded {
    /// 汉字换成全拼、字间留空格，其余字符照抄：`github xing biao `。
    spaced: String,
    /// 只留字母数字的全拼连写：`githubxingbiao`。
    tight: String,
    /// 首字母，英文留整词：`githubxb`。
    mixed: String,
    /// 首字母，英文也缩成一个字母：`gxb`。
    initials: String,
}

impl Folded {
    /// 这一段是不是出现在展开后的文本里。
    pub fn contains(&self, token: &str) -> bool {
        self.spaced.contains(token)
            || self.tight.contains(token)
            || self.mixed.contains(token)
            || self.initials.contains(token)
    }
}

/// 这个搜索词能不能命中这段文本。
///
/// 搜索词按空白切成若干段，每一段都要命中；每一段先当原文找（不区分大小写），
/// 找不到再看拼音的全拼和首字母。
pub fn matches(haystack: &str, query: &str) -> bool {
    let query = query.trim();
    if query.is_empty() {
        return true;
    }
    let lower = haystack.to_lowercase();
    let mut folded: Option<Folded> = None;
    query.split_whitespace().all(|token| {
        let token = token.to_lowercase();
        if lower.contains(&token) {
            return true;
        }
        // 搜索词里有汉字：拼音串里不会有汉字，原文没命中就没戏，不必展开。
        if token.chars().any(is_han) {
            return false;
        }
        folded.get_or_insert_with(|| fold(&lower)).contains(&token)
    })
}

/// 展开成拼音形态。`text` 不必先小写——查表与大小写无关，大小写在这里统一抹平。
///
/// 「英文词」指连续的字母数字：整词进 `mixed`，只有词首进 `initials`。
/// 汉字的每个首字母两种串都进。
fn fold(text: &str) -> Folded {
    let mut spaced = String::with_capacity(text.len() * 4);
    let mut tight = String::with_capacity(text.len() * 4);
    let mut mixed = String::with_capacity(text.len() * 2);
    let mut initials = String::with_capacity(text.len() * 2);
    // 上一个字符是不是字母数字：用来判断「英文词」的边界。
    let mut in_word = false;
    for ch in text.chars() {
        if let Some(pinyin) = ch.to_pinyin() {
            let plain = pinyin.plain();
            spaced.push_str(plain);
            spaced.push(' ');
            tight.push_str(plain);
            mixed.push_str(pinyin.first_letter());
            initials.push_str(pinyin.first_letter());
            in_word = false;
            continue;
        }
        for lower in ch.to_lowercase() {
            if is_han(lower) {
                // 没有拼音数据的生僻字：字面留在各形态里，至少不会凭空消失。
                spaced.push(lower);
                tight.push(lower);
                mixed.push(lower);
                initials.push(lower);
                in_word = false;
            } else if lower.is_alphanumeric() {
                spaced.push(lower);
                tight.push(lower);
                mixed.push(lower);
                if !in_word {
                    initials.push(lower);
                }
                in_word = true;
            } else if lower.is_whitespace() {
                spaced.push(' ');
                in_word = false;
            } else {
                // 标点只留在原文形态里；连写和首字母串不该被 `-` `.` 打断。
                spaced.push(lower);
                in_word = false;
            }
        }
    }
    Folded {
        spaced,
        tight,
        mixed,
        initials,
    }
}

/// 汉字（含扩展区和兼容区）。`char::is_alphanumeric` 对汉字也返回真，
/// 所以不能只靠它区分。
fn is_han(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3400..=0x4dbf | 0x4e00..=0x9fff | 0xf900..=0xfaff | 0x20000..=0x2fa1f
    )
}

#[cfg(test)]
mod tests {
    use super::{fold, matches};

    #[test]
    fn chinese_full_pinyin() {
        let text = "打开设置";
        assert!(matches(text, "dakaishezhi"));
        assert!(matches(text, "dakai"));
        assert!(matches(text, "shezhi"));
        assert!(matches(text, "kai"));
        assert!(!matches(text, "guanbi"));
    }

    #[test]
    fn chinese_initials() {
        let text = "打开设置";
        assert!(matches(text, "dksz"));
        assert!(matches(text, "sz")); // 词尾那一截首拼也认
        assert!(!matches(text, "dkbz"));
    }

    #[test]
    fn original_chinese_still_works() {
        let text = "打开设置";
        assert!(matches(text, "设置"));
        assert!(matches(text, "打开"));
        assert!(!matches(text, "关闭"));
    }

    #[test]
    fn mixed_chinese_english() {
        let text = "GitHub 星标";
        assert!(matches(text, "github"));
        assert!(matches(text, "GitHub"));
        assert!(matches(text, "星标"));
        assert!(matches(text, "xb"));
        assert!(matches(text, "xingbiao"));
        assert!(matches(text, "githubxb"));
        assert!(matches(text, "gxb"));
        assert!(matches(text, "githubxingbiao"));
        assert!(matches(text, "github xing"));
        assert!(!matches(text, "gitlab"));
    }

    #[test]
    fn english_leads_and_chinese_follows() {
        let text = "VSCode 打开文件夹";
        assert!(matches(text, "vscode"));
        assert!(matches(text, "dkwjj"));
        assert!(matches(text, "dakaiwenjianjia"));
        assert!(matches(text, "vdkwjj"));
        assert!(!matches(text, "sublime"));
    }

    #[test]
    fn path_and_punctuation_do_not_break_typing() {
        let text = "C:\\Users\\cz\\Desktop\\喝水提醒.exe";
        assert!(matches(text, "喝水提醒"));
        assert!(matches(text, "hstx"));
        assert!(matches(text, "heshuitixing"));
        assert!(matches(text, "users"));
        assert!(matches(text, "cu"));
    }

    #[test]
    fn ascii_only_behaves_like_before() {
        assert!(matches("Visual Studio Code", "vsc"));
        assert!(matches("Visual Studio Code", "studio"));
        assert!(matches("Visual Studio Code", "visualstudio"));
        assert!(matches("Visual Studio Code", "vs code"));
        assert!(!matches("Visual Studio Code", "vim"));
    }

    #[test]
    fn multi_token_query_needs_every_token() {
        assert!(matches("GitHub 星标", "github xb"));
        assert!(matches("GitHub 星标", "星标 xb"));
        assert!(!matches("GitHub 星标", "github dksz"));
    }

    #[test]
    fn empty_query_matches_everything() {
        assert!(matches("随便什么", ""));
        assert!(matches("随便什么", "   "));
    }

    #[test]
    fn shape_of_a_mixed_string() {
        let folded = fold("GitHub 星标");
        assert_eq!(folded.spaced, "github xing biao ");
        assert_eq!(folded.tight, "githubxingbiao");
        assert_eq!(folded.mixed, "githubxb");
        assert_eq!(folded.initials, "gxb");
    }

    /// 拿程序里真实在用的文案试，别只在人造字符串上过。
    #[test]
    fn real_catalogue_text() {
        let text = "列出自己的 GitHub 仓库，先读缓存";
        assert!(matches(text, "github"));
        assert!(matches(text, "仓库"));
        assert!(matches(text, "cangku"));
        assert!(matches(text, "githubck"));

        let text = "打开计算器，算表达式的值";
        assert!(matches(text, "jsq"));
        assert!(matches(text, "jisuanqi"));
        assert!(matches(text, "bds"));

        let text = "打开启动器设置";
        assert!(matches(text, "qdqsz"));
        assert!(matches(text, "qidongqishezhi"));
    }
}
