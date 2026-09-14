//! 危险命令检测引擎（F6.1）：执行前所有 AI 生成命令必经规则引擎。
//! - 规则分级 high/medium/low，规则表外置 JSON 配置文件，支持热加载（实现注意事项 #8）。
//! - 基于命令解析（命令主体 + 参数），非简单子串匹配。
//! - AI 风险标注与规则引擎冲突时取更高等级。

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::schema::RiskLevel;

/// 单条危险规则。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DangerRule {
    pub id: String,
    #[serde(rename = "level")]
    pub level: RiskLevel,
    /// 命令名精确匹配；"*" 匹配任意命令（配合 pattern 使用）
    pub command: String,
    /// 要求的短标志字符集，如 "rf" 表示需包含 -r 与 -f（含组合 -rf/-fr）
    #[serde(default)]
    pub short_flags: String,
    /// 要求的长标志（如 "force" → --force）
    #[serde(default)]
    pub long_flags: Vec<String>,
    /// 至少出现其中一个的精确参数（如 "stop"、"777"）
    #[serde(default)]
    pub args_any: Vec<String>,
    /// 附加正则，作用于整条命令（可选）
    #[serde(default)]
    pub pattern: Option<String>,
    /// 命中原因（中文，供前端展示）
    pub reason: String,
}

/// 规则表（JSON 顶层结构）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleTable {
    pub version: u32,
    pub rules: Vec<DangerRule>,
}

/// 安全检查结论。
#[derive(Debug, Clone, Serialize)]
pub struct SafetyVerdict {
    pub level: RiskLevel,
    /// 命中的规则原因列表
    pub reasons: Vec<String>,
    /// 命中的规则 ID 列表
    pub matched_rules: Vec<String>,
}

impl SafetyVerdict {
    pub fn safe() -> Self {
        Self {
            level: RiskLevel::Low,
            reasons: Vec::new(),
            matched_rules: Vec::new(),
        }
    }
}

/// 默认规则表（内置；可通过 reload_from 以外部 JSON 热替换）。
pub fn default_rules() -> RuleTable {
    serde_json::from_str(include_str!("rules/danger_rules.json"))
        .expect("内置危险规则表必须合法")
}

#[derive(Debug)]
pub struct SafetyGuard {
    table: RuleTable,
}

impl Default for SafetyGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl SafetyGuard {
    pub fn new() -> Self {
        Self {
            table: default_rules(),
        }
    }

    /// 热加载规则表（实现注意事项 #8：外置 JSON + 热加载）。
    pub fn reload_from(&mut self, path: &str) -> Result<()> {
        let raw = std::fs::read_to_string(path).map_err(Error::Io)?;
        let table: RuleTable = serde_json::from_str(&raw)?;
        // 校验：level 合法由 serde 保证；至少要有规则
        if table.rules.is_empty() {
            return Err(Error::Safety("规则表为空".into()));
        }
        self.table = table;
        Ok(())
    }

    /// 对单条命令做安全检查；ai_risk 为 LLM 标注的风险等级（可空）。
    pub fn check(&self, command: &str, ai_risk: Option<RiskLevel>) -> SafetyVerdict {
        let tokens = tokenize(command);
        let cmd = resolve_command(&tokens);
        let mut verdict = SafetyVerdict::safe();
        let mut level = RiskLevel::Low;

        for rule in &self.table.rules {
            if self.match_rule(rule, &cmd, &tokens, command) {
                level = level.max(rule.level);
                verdict.reasons.push(rule.reason.clone());
                verdict.matched_rules.push(rule.id.clone());
            }
        }
        if let Some(ai) = ai_risk {
            level = level.max(ai);
            if ai != RiskLevel::Low
                && !verdict
                    .reasons
                    .iter()
                    .any(|r| r.contains("AI 标注"))
            {
                verdict.reasons.push(format!("AI 标注风险等级: {ai:?}"));
            }
        }
        verdict.level = level;
        verdict
    }

    fn match_rule(&self, rule: &DangerRule, cmd: &str, tokens: &[String], full: &str) -> bool {
        // 命令名
        if rule.command != "*" && rule.command != cmd {
            return false;
        }
        // 短标志字符集
        for ch in rule.short_flags.chars() {
            if !tokens.iter().any(|t| is_short_flag_cluster(t, ch)) {
                return false;
            }
        }
        // 长标志
        for lf in &rule.long_flags {
            let needle = if lf.starts_with("--") { lf.clone() } else { format!("--{lf}") };
            if !tokens.iter().any(|t| t == &needle) {
                return false;
            }
        }
        // 参数任一命中
        if !rule.args_any.is_empty() && !rule.args_any.iter().any(|a| tokens.iter().any(|t| t == a)) {
            return false;
        }
        // 附加正则
        if let Some(p) = &rule.pattern {
            let re = match regex::Regex::new(p) {
                Ok(r) => r,
                Err(_) => return false, // 非法规则不误伤
            };
            if !re.is_match(full) {
                return false;
            }
        }
        true
    }
}

/// 是否为包含指定字符的短标志簇（-rf 含 r 和 f；--xxx 不算短标志）。
fn is_short_flag_cluster(token: &str, ch: char) -> bool {
    let t = token.strip_prefix('-').unwrap_or(token);
    if t.is_empty() || t.starts_with('-') || t.starts_with('=') {
        return false;
    }
    t.chars().all(|c| c.is_ascii_alphanumeric() || c == '=') && t.chars().any(|c| c == ch)
}

/// 简单 shell 分词：处理单双引号与反斜杠转义。
pub fn tokenize(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut cur = String::new();
    let mut chars = input.chars().peekable();
    let mut quote: Option<char> = None;
    while let Some(c) = chars.next() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                } else if c == '\\' && q == '"' {
                    cur.push('\\');
                    if let Some(n) = chars.next() {
                        cur.push(n);
                    }
                } else {
                    cur.push(c);
                }
            }
            None => match c {
                '\'' | '"' => quote = Some(c),
                '\\' => {
                    if let Some(n) = chars.next() {
                        cur.push(n);
                    }
                }
                c if c.is_whitespace() => {
                    if !cur.is_empty() {
                        tokens.push(std::mem::take(&mut cur));
                    }
                }
                ';' | '|' | '&' | '(' | ')' | '>' | '<' | '\n' => {
                    if !cur.is_empty() {
                        tokens.push(std::mem::take(&mut cur));
                    }
                    tokens.push(c.to_string());
                }
                _ => cur.push(c),
            },
        }
    }
    if !cur.is_empty() {
        tokens.push(cur);
    }
    tokens
}

/// 解析命令主体：跳过 sudo/env/nohup 等前缀，取首个可执行名（去路径）。
fn resolve_command(tokens: &[String]) -> String {
    const PREFIX: &[&str] = &["sudo", "env", "nohup", "time", "command", "exec", "nice", "watch"];
    let mut idx = 0;
    while idx < tokens.len() {
        let t = &tokens[idx];
        if PREFIX.contains(&t.as_str()) {
            // sudo -u root 这种带参数的前缀也要跳过
            idx += 1;
            if t == "sudo" && idx < tokens.len() && tokens[idx].starts_with('-') {
                while idx < tokens.len() && tokens[idx].starts_with('-') {
                    idx += 1;
                    // -u 需要值
                    if tokens[idx.saturating_sub(1)] == "-u" || tokens[idx.saturating_sub(1)] == "--user" {
                        idx += 1;
                    }
                }
            }
            if t == "env" && idx < tokens.len() && tokens[idx].contains('=') {
                while idx < tokens.len() && tokens[idx].contains('=') {
                    idx += 1;
                }
            }
            continue;
        }
        break;
    }
    let name = tokens.get(idx).cloned().unwrap_or_default();
    name.rsplit('/').next().unwrap_or(&name).to_string()
}

/// 组合检查：AI 命令执行前调用（F3.2 第 6 步，无绕过路径）。
pub fn safety_check(command: &str, ai_risk: Option<RiskLevel>) -> SafetyVerdict {
    SafetyGuard::new().check(command, ai_risk)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_quotes() {
        assert_eq!(tokenize("echo 'a b' c"), vec!["echo", "a b", "c"]);
        assert_eq!(tokenize(r#"ls -la "/tmp/x y""#), vec!["ls", "-la", "/tmp/x y"]);
    }

    #[test]
    fn resolve_sudo_and_path() {
        let t = tokenize("sudo -u root /bin/rm -rf /tmp/x");
        assert_eq!(resolve_command(&t), "rm");
        let t = tokenize("systemctl stop nginx");
        assert_eq!(resolve_command(&t), "systemctl");
    }

    #[test]
    fn high_rm_root() {
        let v = safety_check("rm -rf /", None);
        assert_eq!(v.level, RiskLevel::High, "{:?}", v.reasons);
    }

    #[test]
    fn high_sudo_rm_system_dir() {
        let v = safety_check("sudo rm -fr /etc", None);
        assert_eq!(v.level, RiskLevel::High, "{:?}", v.reasons);
    }

    #[test]
    fn medium_rm_dir() {
        let v = safety_check("rm -r ./build", None);
        assert_eq!(v.level, RiskLevel::Medium, "{:?}", v.reasons);
    }

    #[test]
    fn low_plain_command() {
        let v = safety_check("ls -la", None);
        assert_eq!(v.level, RiskLevel::Low);
    }

    #[test]
    fn high_mkfs_and_dd() {
        assert_eq!(safety_check("mkfs.ext4 /dev/sda1", None).level, RiskLevel::High);
        assert_eq!(safety_check("dd if=/dev/zero of=/dev/sda bs=1M", None).level, RiskLevel::High);
    }

    #[test]
    fn high_power_commands() {
        assert_eq!(safety_check("shutdown -h now", None).level, RiskLevel::High);
        assert_eq!(safety_check("reboot", None).level, RiskLevel::High);
    }

    #[test]
    fn high_drop_database() {
        assert_eq!(safety_check("mysql -e 'DROP DATABASE app;'", None).level, RiskLevel::High);
    }

    #[test]
    fn high_fork_bomb() {
        assert_eq!(safety_check(":(){ :|:& };:", None).level, RiskLevel::High);
    }

    #[test]
    fn medium_kill_and_chmod_and_systemctl() {
        assert_eq!(safety_check("kill -9 1234", None).level, RiskLevel::Medium);
        assert_eq!(safety_check("chmod 777 /etc/passwd", None).level, RiskLevel::Medium);
        assert_eq!(safety_check("systemctl stop nginx", None).level, RiskLevel::Medium);
    }

    #[test]
    fn ai_risk_takes_max() {
        let v = safety_check("echo hi", Some(RiskLevel::High));
        assert_eq!(v.level, RiskLevel::High);
        let v = safety_check("rm -r /tmp/x", Some(RiskLevel::Low));
        assert_eq!(v.level, RiskLevel::Medium);
    }

    #[test]
    fn hot_reload() {
        let mut g = SafetyGuard::new();
        let path = std::env::temp_dir().join("ai-ssh-test-rules.json");
        let table = serde_json::json!({
            "version": 1,
            "rules": [{
                "id": "test-only", "level": "high", "command": "dangerous-cmd",
                "short_flags": "", "long_flags": [], "args_any": [], "pattern": null,
                "reason": "测试规则"
            }]
        });
        std::fs::write(&path, table.to_string()).unwrap();
        g.reload_from(path.to_str().unwrap()).unwrap();
        assert_eq!(g.check("dangerous-cmd --x", None).level, RiskLevel::High);
        assert_eq!(g.check("ls", None).level, RiskLevel::Low);
    }
}
