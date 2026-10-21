//! 表达式求值：只认 `+ - * / ^` 五种运算（外加括号分组、小数和一元正负号）。
//!
//! 用户随手打一串算式，能算出结果就显示，不能算就什么都不显示——所以这里对
//! 「什么算能算」很宽容：半角全角符号、空格换行都认，算到一半（`1+`）、除零、
//! 溢出的一律返回 `None`，由界面决定不画结果。

/// 把 `expression` 算成一个数；算不了（含除零、溢出、NaN）就是 `None`。
pub fn evaluate(expression: &str) -> Option<f64> {
    let chars = normalize(expression);
    if chars.is_empty() {
        return None;
    }
    let mut parser = Parser { chars, pos: 0 };
    let value = parser.sum()?;
    // 必须整个吃完：`1+2)` 这种剩一半的不算。算出来不是有限的数也不算。
    (parser.pos == parser.chars.len() && value.is_finite()).then_some(value)
}

/// 一个数该显示成的样子：整数不带小数点，小数截到十位、尾巴上的零去掉。
pub fn format(value: f64) -> String {
    if !value.is_finite() {
        return String::new();
    }
    if value == value.trunc() && value.abs() < 1e15 {
        return format!("{}", value as i64);
    }
    let text = format!("{value:.10}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" {
        // -0.0 截断后剩下的空壳，没有显示价值。
        return "0".to_string();
    }
    text.to_string()
}

/// 算式算出结果后该显示的字；算不了（含除零、溢出、NaN）就是 `None`。
pub fn result_text(expression: &str) -> Option<String> {
    // 算不出的都已经在 `format` 里变成空串：非有限值没有可显示的样子。
    let text = format(evaluate(expression)?);
    (!text.is_empty()).then_some(text)
}

/// 全角符号换成半角，再把所有空白剔掉。
///
/// 中文输入法下 `+ - * / ^` 经常出来的是全角，数字、括号和小数点也是；不换的话
/// 用户明明打对了却算不出结果。
fn normalize(expression: &str) -> Vec<char> {
    expression
        .chars()
        .filter_map(|c| match c {
            '＋' | '﹢' => Some('+'),
            '－' | '−' => Some('-'),
            '＊' | '×' | '·' => Some('*'),
            '／' | '÷' => Some('/'),
            '＾' => Some('^'),
            '（' => Some('('),
            '）' => Some(')'),
            '．' | '，' => Some('.'),
            c if c.is_whitespace() => None,
            // 全角数字：中文输入法打出来的一字数。
            '０'..='９' => Some((b'0' + (c as u32 - '０' as u32) as u8) as char),
            c => Some(c),
        })
        .collect()
}

/// 递归下降求值器。文法（从松到紧）：
///
/// ```text
/// sum     = product (('+' | '-') product)*
/// product = unary (('*' | '/') unary)*
/// unary   = ('+' | '-') unary | power
/// power   = atom ('^' unary)?          // 右结合，且允许 2^-1
/// atom    = number | '(' sum ')'
/// ```
struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn sum(&mut self) -> Option<f64> {
        let mut value = self.product()?;
        loop {
            match self.peek() {
                Some('+') => {
                    self.pos += 1;
                    value += self.product()?;
                }
                Some('-') => {
                    self.pos += 1;
                    value -= self.product()?;
                }
                _ => return Some(value),
            }
        }
    }

    fn product(&mut self) -> Option<f64> {
        let mut value = self.unary()?;
        loop {
            match self.peek() {
                Some('*') => {
                    self.pos += 1;
                    value *= self.unary()?;
                }
                Some('/') => {
                    self.pos += 1;
                    let divisor = self.unary()?;
                    if divisor == 0.0 {
                        return None;
                    }
                    value /= divisor;
                }
                _ => return Some(value),
            }
        }
    }

    fn unary(&mut self) -> Option<f64> {
        match self.peek() {
            Some('-') => {
                self.pos += 1;
                Some(-self.unary()?)
            }
            Some('+') => {
                self.pos += 1;
                self.unary()
            }
            _ => self.power(),
        }
    }

    fn power(&mut self) -> Option<f64> {
        let base = self.atom()?;
        if self.peek() == Some('^') {
            self.pos += 1;
            // 右结合：指数那边还能再带正负号，所以递归的是 unary 而不是 atom。
            let exponent = self.unary()?;
            return Some(base.powf(exponent));
        }
        Some(base)
    }

    fn atom(&mut self) -> Option<f64> {
        match self.peek()? {
            '(' => {
                self.pos += 1;
                let value = self.sum()?;
                (self.peek() == Some(')')).then(|| {
                    self.pos += 1;
                    value
                })
            }
            _ => self.number(),
        }
    }

    fn number(&mut self) -> Option<f64> {
        let start = self.pos;
        let mut digits = false;
        let mut dot = false;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                digits = true;
                self.pos += 1;
            } else if c == '.' && !dot {
                dot = true;
                self.pos += 1;
            } else {
                break;
            }
        }
        if !digits {
            return None;
        }
        self.chars[start..self.pos]
            .iter()
            .collect::<String>()
            .parse()
            .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 算得出来，并且正是这个数。
    fn assert_value(expression: &str, expected: f64) {
        assert_eq!(evaluate(expression), Some(expected), "算式：{expression}");
    }

    /// 算不出来。
    fn assert_none(expression: &str) {
        assert_eq!(evaluate(expression), None, "算式：{expression}");
    }

    #[test]
    fn plain_arithmetic() {
        assert_value("1+1", 2.0);
        assert_value("41+1", 42.0);
        assert_value("7-9", -2.0);
        assert_value("6*7", 42.0);
        assert_value("84/2", 42.0);
        assert_value("2^10", 1024.0);
    }

    #[test]
    fn multiplication_binds_tighter_than_addition() {
        assert_value("1+2*3", 7.0);
        assert_value("1+2*3+4", 11.0);
        assert_value("10-2*3", 4.0);
        assert_value("100/5/2", 10.0);
    }

    #[test]
    fn power_binds_tighter_than_multiplication() {
        assert_value("2*3^2", 18.0);
        assert_value("2^3*4", 32.0);
    }

    #[test]
    fn power_is_right_associative() {
        assert_value("2^3^2", 512.0);
    }

    #[test]
    fn unary_minus_sits_below_power() {
        assert_value("-2^2", -4.0);
        assert_value("2^-1", 0.5);
        assert_value("--3", 3.0);
        assert_value("-(1+1)", -2.0);
    }

    #[test]
    fn parentheses_group() {
        assert_value("(1+2)*3", 9.0);
        assert_value("((2))", 2.0);
        assert_value("2*(3+(4-1))", 12.0);
    }

    #[test]
    fn decimals_and_leading_dot() {
        assert_value("0.1+0.2", 0.1 + 0.2);
        assert_value(".5*2", 1.0);
        assert_value("1.", 1.0);
    }

    #[test]
    fn full_width_symbols_and_spaces_are_accepted() {
        assert_value("１＋１", 2.0);
        assert_value("1＋2*3", 7.0);
        assert_value("（1+1）*2", 4.0);
        assert_value(" 1 + 1 ", 2.0);
        assert_value("1\n+\t1", 2.0);
    }

    #[test]
    fn unfinished_expressions_have_no_result() {
        assert_none("");
        assert_none("   ");
        assert_none("1+");
        assert_none("*3");
        assert_none("(1+2");
        assert_none("1+2)");
        assert_none("()");
        assert_none("abc");
        assert_none("1&2");
    }

    #[test]
    fn division_by_zero_has_no_result() {
        assert_none("1/0");
        assert_none("5/(3-3)");
    }

    #[test]
    fn overflow_and_nan_have_no_result() {
        assert_none("1/0*0");
        assert_none("(-1)^0.5");
        assert_none("10^10^10");
    }

    #[test]
    fn integers_print_without_a_decimal_point() {
        assert_eq!(format(41.0), "41");
        assert_eq!(format(-2.0), "-2");
        assert_eq!(format(0.5), "0.5");
        assert_eq!(format(-0.0), "0");
    }

    #[test]
    fn decimals_are_trimmed_not_dumped_raw() {
        // 二进制浮点的毛边不该原样摆到用户面前。
        assert_eq!(format(0.1 + 0.2), "0.3");
        assert_eq!(format(1.0 / 3.0), "0.3333333333");
        assert_eq!(format(2.0_f64.powf(-1.0)), "0.5");
    }

    #[test]
    fn result_text_hides_everything_that_is_not_a_number() {
        assert_eq!(result_text("1+1").as_deref(), Some("2"));
        assert_eq!(result_text("1/0"), None);
        assert_eq!(result_text("nope"), None);
        assert_eq!(result_text(""), None);
    }
}
