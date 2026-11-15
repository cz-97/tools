//! 拼音搜索：把文本拆成「音节序列」，搜索词按音节**前缀**匹配。
//!
//! 规则照输入法的找字习惯来：
//!
//! 1. 每个汉字是一个音节（全拼），连续的一串字母数字是一个音节；
//! 2. 搜索词可以从**任意一个音节的开头**开始——`bao` 能搜到「周报」；
//! 3. 但**不能从音节中间开始**——`oub`、`houb` 搜不到「周报」，它们是从
//!    `zhoubao` 中间切出来的；
//! 4. 每一段取的是音节的前缀——`zho` 能匹配 `zhong`，`ho` / `hon` / `hong` 都不行。
//!
//! 于是「周报」能命中的是 `zhoubao` / `zhouba` / `zhoub` / `zho` / `zh` / `zb` /
//! `bao` / `b`，而不是 `ub` / `oub` / `ouba` / `houb`。
//!
//! 原文匹配（不区分大小写的子串）在 [`matches`] 里先做：纯英文、纯中文的搜索
//! 一分钱不花，拼音只是原文找不到时的第二条路。

use pinyin::ToPinyin;

/// 一段文本展开后的音节序列。
struct Folded {
    /// 每个汉字一个全拼音节；连续字母数字一个音节。标点、空格只是边界。
    syllables: Vec<String>,
}

impl Folded {
    /// 搜索词能不能按「音节前缀」匹配上。
    ///
    /// 用一张 `dp[i][j]` 表：前 `i` 个音节能不能吃掉搜索词的前 `j` 个字符。
    /// 每个音节只能从它的第 0 个字符开始吃，吃 1..=音节长度 个字符（前缀）。
    fn matches_token(&self, token: &str) -> bool {
        let chars: Vec<char> = token.chars().collect();
        if chars.is_empty() {
            return true;
        }
        let count = self.syllables.len();
        let width = chars.len();
        let mut dp = vec![vec![false; width + 1]; count + 1];
        // 从任意音节开始都算数（`bao` 要能搜到「周报」）。
        for row in dp.iter_mut() {
            row[0] = true;
        }
        for (index, syllable) in self.syllables.iter().enumerate() {
            let unit: Vec<char> = syllable.chars().collect();
            for taken in 0..=width {
                if !dp[index][taken] {
                    continue;
                }
                let room = unit.len().min(width - taken);
                for step in 1..=room {
                    // 跨过音节的第一个字符就不是前缀了，后面更长的也不用看。
                    if chars[taken + step - 1] != unit[step - 1] {
                        break;
                    }
                    dp[index + 1][taken + step] = true;
                }
            }
        }
        // 搜索词吃完就算命中：后面还有没有音节不管（`zho` 只用到「周」）。
        dp.iter().any(|row| row[width])
    }
}

/// 这个搜索词能不能命中这段文本。
///
/// 搜索词按空白切成若干段，每一段都要命中；每一段先当原文找（不区分大小写），
/// 找不到再按音节前缀匹配拼音。
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
        folded.get_or_insert_with(|| fold(&lower)).matches_token(&token)
    })
}

/// 拆成音节序列。
///
/// 汉字取全拼（`plain()` 与大小写无关）；连续字母数字整段算一个音节（`vscode`
/// 是一个，不是六个）；其余字符只当边界，不进序列。
fn fold(text: &str) -> Folded {
    let mut syllables: Vec<String> = Vec::new();
    let mut word = String::new();
    for ch in text.chars() {
        if let Some(pinyin) = ch.to_pinyin() {
            push_word(&mut word, &mut syllables);
            syllables.push(pinyin.plain().to_string());
            continue;
        }
        if ch.is_alphanumeric() {
            word.extend(ch.to_lowercase());
            continue;
        }
        // 标点、空格、没有拼音数据的生僻字：都是音节边界。
        push_word(&mut word, &mut syllables);
    }
    push_word(&mut word, &mut syllables);
    Folded { syllables }
}

fn push_word(word: &mut String, syllables: &mut Vec<String>) {
    if !word.is_empty() {
        syllables.push(std::mem::take(word));
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

    /// 用户报过的那个例子：`ub` 是 `zhoubao` 中间的两个字母，不该命中。
    #[test]
    fn a_syllable_only_matches_from_its_first_letter() {
        let text = "周报";
        // 前序完整拼音、前序拼音、首拼都认
        assert!(matches(text, "zhoubao"));
        assert!(matches(text, "zhouba"));
        assert!(matches(text, "zhoub"));
        assert!(matches(text, "zho"));
        assert!(matches(text, "zh"));
        assert!(matches(text, "zb"));
        assert!(matches(text, "bao")); // 从第二个字开始也认
        assert!(matches(text, "b"));
        // 从音节中间切出来的都不认
        assert!(!matches(text, "ub"));
        assert!(!matches(text, "oub"));
        assert!(!matches(text, "ouba"));
        assert!(!matches(text, "houb"));
        assert!(!matches(text, "houba"));
    }

    #[test]
    fn a_single_syllable_must_start_at_the_beginning() {
        let text = "中文";
        assert!(matches(text, "zhong"));
        assert!(matches(text, "zho"));
        assert!(matches(text, "zhongw"));
        assert!(!matches(text, "hong"));
        assert!(!matches(text, "hon"));
        assert!(!matches(text, "ong"));
    }

    #[test]
    fn shape_of_a_mixed_string() {
        let folded = fold("github 星标");
        assert_eq!(folded.syllables, ["github", "xing", "biao"]);
        // 标点和空格只是边界，不产生空音节。
        let folded = fold("C:\\Users\\cz\\Desktop\\喝水提醒.exe");
        assert_eq!(
            folded.syllables,
            ["c", "users", "cz", "desktop", "he", "shui", "ti", "xing", "exe"]
        );
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
