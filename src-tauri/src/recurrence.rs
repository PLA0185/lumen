//! 重复规则引擎（任务书 §5，独立核心模块）。
//!
//! ## 为什么不直接用 rrule crate
//!
//! 调研发现 `rrule` 0.14.0 已停更 17 个月，且它的月末处理策略是库内定的、
//! 无法按任务书要求"定义一致的处理策略并展示给用户"。任务书 §5 明确要求
//! 月末、闰年、跨时区、夏令时和"每月 31 日"都必须有**我们自己定义并展示**
//! 的策略。因此这里实现一个聚焦的规则引擎：只覆盖任务书列出的规则类型，
//! 但每一条边界行为都是显式的、可测试的、可向用户解释的。
//!
//! ## 存储与计算的分工
//!
//! - 数据库存 `RRULE` 字符串（RFC 5545 风格）+ `tzid` + `dtstart_local`（本地墙上时间）；
//! - 展开时按**本地墙上时间**逐次推进，再按 tzid 转成 UTC。
//!
//! 这样做的原因：重复任务的语义是"每天/每周的那个墙上时刻"，
//! 而不是"每隔 86400 秒"。跨夏令时时，墙上时刻不变才是用户期望的行为。
//!
//! ## 明确的边界策略（都会在界面上展示）
//!
//! | 情况 | 策略 |
//! | --- | --- |
//! | 每月 31 日，遇到只有 30 天的月份 | **跳过该月**（不挪到 30 日） |
//! | 每月 29/30/31 日，遇到 2 月 | **跳过**（闰年 2 月 29 日存在时正常发生） |
//! | 每年 2 月 29 日，平年 | **跳过**（只在闰年发生） |
//! | 第 5 个星期 X，该月不足 5 个 | **跳过该月** |
//! | 结束条件 | `永不` / `直到某日` / `共 N 次`，三者互斥 |

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

/// 重复频率
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Freq {
    /// 每天
    Daily,
    /// 每周
    Weekly,
    /// 每月
    Monthly,
    /// 每年
    Yearly,
}

impl Freq {
    fn as_rrule(self) -> &'static str {
        match self {
            Self::Daily => "DAILY",
            Self::Weekly => "WEEKLY",
            Self::Monthly => "MONTHLY",
            Self::Yearly => "YEARLY",
        }
    }

    fn from_rrule(s: &str) -> AppResult<Self> {
        match s.to_ascii_uppercase().as_str() {
            "DAILY" => Ok(Self::Daily),
            "WEEKLY" => Ok(Self::Weekly),
            "MONTHLY" => Ok(Self::Monthly),
            "YEARLY" => Ok(Self::Yearly),
            other => Err(AppError::validation(format!("不支持的重复频率：{other}"))
                .with_hint("支持 DAILY / WEEKLY / MONTHLY / YEARLY")),
        }
    }
}

/// 结束条件
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EndCondition {
    /// 永不结束（默认）
    #[default]
    Never,
    /// 直到某个本地日期（含当日）
    Until {
        /// `YYYY-MM-DD`
        date: String,
    },
    /// 共发生 N 次
    Count {
        /// 次数
        count: i64,
    },
}

/// 一条完整的重复规则（对外的结构化表示）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecurrenceRule {
    /// 频率
    pub freq: Freq,
    /// 间隔：每 N 天/周/月/年（>= 1）
    pub interval: i64,
    /// 指定星期（1=周一 … 7=周日）；仅 WEEKLY 使用，空表示用起始日的星期
    pub by_weekday: Vec<u8>,
    /// 每月第几天的列表（1–31）；仅 MONTHLY 使用，空表示用起始日的日
    /// 每月几号。**允许负数**：`-1` 表示当月最后一天（RFC 5545 语义），
    /// `-2` 表示倒数第二天，以此类推。短月与闰年由展开时按当月实际天数解析。
    pub by_monthday: Vec<i8>,
    /// 每月第 N 个星期 X（1–5 表示第几个，-1 表示最后一个）；与 by_monthday 互斥
    pub by_setpos: Option<SetPos>,
    /// 仅 YEARLY：月份（1–12），空表示用起始日的月份
    pub by_month: Vec<u8>,
    /// 是否只取工作日（周一至周五）
    pub weekdays_only: bool,
    /// 结束条件
    pub end: EndCondition,
    /// 时区标识，如 `Asia/Shanghai`
    pub tzid: String,
    /// 首次发生的本地墙上时间 `YYYY-MM-DDTHH:MM:SS`
    pub dtstart_local: String,
    /// 是否含具体时刻（false 表示仅日期）
    pub has_start_time: bool,
}

/// 每月第 N 个星期 X
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetPos {
    /// 第几个（1–5），或 -1 表示最后一个
    pub nth: i8,
    /// 星期（1=周一 … 7=周日）
    pub weekday: u8,
}

// =============================================================================
// 与 RRULE 字符串互转
// =============================================================================

/// 星期编号 ↔ RRULE 的 BYDAY 代码
fn weekday_to_code(w: u8) -> AppResult<&'static str> {
    Ok(match w {
        1 => "MO",
        2 => "TU",
        3 => "WE",
        4 => "TH",
        5 => "FR",
        6 => "SA",
        7 => "SU",
        other => {
            return Err(
                AppError::validation(format!("星期编号非法：{other}")).with_hint("1=周一 … 7=周日")
            )
        }
    })
}

fn code_to_weekday(s: &str) -> AppResult<u8> {
    Ok(match s.to_ascii_uppercase().as_str() {
        "MO" => 1,
        "TU" => 2,
        "WE" => 3,
        "TH" => 4,
        "FR" => 5,
        "SA" => 6,
        "SU" => 7,
        other => return Err(AppError::validation(format!("不支持的星期代码：{other}"))),
    })
}

impl RecurrenceRule {
    /// 序列化为 RRULE 字符串（不含 DTSTART，DTSTART 单独存储在 dtstart_local）
    pub fn to_rrule_string(&self) -> AppResult<String> {
        let mut parts = vec![format!("FREQ={}", self.freq.as_rrule())];

        if self.interval > 1 {
            parts.push(format!("INTERVAL={}", self.interval));
        }

        // 工作日规则用 BYDAY=MO,TU,WE,TH,FR 表达
        if self.weekdays_only {
            parts.push("BYDAY=MO,TU,WE,TH,FR".to_string());
        } else if let Some(sp) = &self.by_setpos {
            // BYSETPOS 的星期由它自带的 weekday 决定，此处**不能**再输出
            // by_weekday，否则会拼出 `BYDAY=FR;BYDAY=FR` 这种重复片段。
            let code = weekday_to_code(sp.weekday)?;
            parts.push(format!("BYDAY={code}"));
            parts.push(format!("BYSETPOS={}", sp.nth));
        } else if !self.by_weekday.is_empty() {
            let mut days: Vec<&str> = Vec::new();
            for w in &self.by_weekday {
                days.push(weekday_to_code(*w)?);
            }
            parts.push(format!("BYDAY={}", days.join(",")));
        }

        // 每月的"第几天"仅在未使用 BYSETPOS 时输出（两者互斥）
        if self.by_setpos.is_none() && !self.by_monthday.is_empty() {
            let days: Vec<String> = self.by_monthday.iter().map(|d| d.to_string()).collect();
            parts.push(format!("BYMONTHDAY={}", days.join(",")));
        }

        if !self.by_month.is_empty() {
            let months: Vec<String> = self.by_month.iter().map(|m| m.to_string()).collect();
            parts.push(format!("BYMONTH={}", months.join(",")));
        }

        // 结束条件必须与 UNTIL / COUNT 互斥
        match &self.end {
            EndCondition::Never => {}
            EndCondition::Until { date } => {
                // RRULE 的 UNTIL 是 UTC 时间戳；这里用当日 23:59:59 以保证"含当日"
                let d = parse_local_date(date)?;
                let end = d
                    .and_hms_opt(23, 59, 59)
                    .ok_or_else(|| AppError::internal("无法构造结束时间"))?;
                parts.push(format!("UNTIL={}T235959Z", end.format("%Y%m%d")));
            }
            EndCondition::Count { count } => {
                if *count < 1 {
                    return Err(AppError::validation("重复次数必须至少为 1"));
                }
                parts.push(format!("COUNT={count}"));
            }
        }

        Ok(parts.join(";"))
    }

    /// 从 RRULE 字符串解析
    pub fn from_rrule_string(
        rrule: &str,
        tzid: &str,
        dtstart_local: &str,
        has_start_time: bool,
    ) -> AppResult<Self> {
        let mut freq: Option<Freq> = None;
        let mut interval: i64 = 1;
        let mut by_weekday: Vec<u8> = Vec::new();
        let mut by_monthday: Vec<i8> = Vec::new();
        let mut by_month: Vec<u8> = Vec::new();
        let mut by_setpos: Option<SetPos> = None;
        let mut weekdays_only = false;
        let mut end = EndCondition::Never;

        for token in rrule.split(';').filter(|s| !s.trim().is_empty()) {
            let (key, value) = token
                .split_once('=')
                .ok_or_else(|| AppError::validation(format!("重复规则片段格式不正确：{token}")))?;
            let key = key.trim().to_ascii_uppercase();
            let value = value.trim();

            match key.as_str() {
                "FREQ" => freq = Some(Freq::from_rrule(value)?),
                "INTERVAL" => {
                    interval = value
                        .parse::<i64>()
                        .map_err(|_| AppError::validation(format!("INTERVAL 不是整数：{value}")))?;
                    if interval < 1 {
                        return Err(AppError::validation("INTERVAL 必须至少为 1"));
                    }
                }
                "BYDAY" => {
                    let codes: Vec<&str> = value.split(',').map(|s| s.trim()).collect();
                    // 识别"工作日"的简写
                    if codes.len() == 5
                        && codes
                            .iter()
                            .all(|c| matches!(*c, "MO" | "TU" | "WE" | "TH" | "FR"))
                    {
                        weekdays_only = true;
                    } else {
                        for c in codes {
                            by_weekday.push(code_to_weekday(c)?);
                        }
                    }
                }
                "BYMONTHDAY" => {
                    for d in value.split(',') {
                        let n: i8 = d.trim().parse().map_err(|_| {
                            AppError::validation(format!("BYMONTHDAY 不是整数：{d}"))
                        })?;
                        // RFC 5545：正数从月初数（1–31），负数从月末倒数
                        // （-1 = 最后一天）。此前只接受正数，导致"每月最后一天"
                        // 这条最常见的需求**根本无法表达**（整改任务书 §14）。
                        let ok = (1..=31).contains(&n) || (-31..=-1).contains(&n);
                        if !ok {
                            return Err(AppError::validation(format!("日期超出范围：{n}"))
                                .with_hint("每月日期可用 1–31（从月初数）或 -1（最后一天）"));
                        }
                        by_monthday.push(n);
                    }
                }
                "BYMONTH" => {
                    for m in value.split(',') {
                        let n: u8 = m
                            .trim()
                            .parse()
                            .map_err(|_| AppError::validation(format!("BYMONTH 不是整数：{m}")))?;
                        if !(1..=12).contains(&n) {
                            return Err(AppError::validation(format!("月份超出范围：{n}"))
                                .with_hint("月份必须在 1–12 之间"));
                        }
                        by_month.push(n);
                    }
                }
                "BYSETPOS" => {
                    let nth: i8 = value
                        .parse()
                        .map_err(|_| AppError::validation(format!("BYSETPOS 不是整数：{value}")))?;
                    // 0 不是合法的"第几个"；范围是 1–5 或 -1（最后一个）
                    if nth == 0 || !(-1..=5).contains(&nth) {
                        return Err(AppError::validation(format!("BYSETPOS 超出范围：{nth}"))
                            .with_hint("允许 1–5（第几个）或 -1（最后一个）"));
                    }
                    // BYSETPOS 必须配合 BYDAY 使用，星期在后续统一校验
                    by_setpos = Some(SetPos { nth, weekday: 1 });
                }
                "UNTIL" => {
                    // 接受 YYYYMMDD 与 YYYYMMDDTHHMMSSZ 两种形式
                    let digits: String = value.chars().take(8).collect();
                    if digits.len() != 8 || !digits.chars().all(|c| c.is_ascii_digit()) {
                        return Err(AppError::validation(format!("UNTIL 格式不正确：{value}"))
                            .with_hint("应形如 20261231 或 20261231T235959Z"));
                    }
                    let date = format!("{}-{}-{}", &digits[0..4], &digits[4..6], &digits[6..8]);
                    parse_local_date(&date)?; // 校验合法性
                    end = EndCondition::Until { date };
                }
                "COUNT" => {
                    let n: i64 = value
                        .parse()
                        .map_err(|_| AppError::validation(format!("COUNT 不是整数：{value}")))?;
                    if n < 1 {
                        return Err(AppError::validation("COUNT 必须至少为 1"));
                    }
                    end = EndCondition::Count { count: n };
                }
                // 未知键不报错：不同工具产生的 RRULE 可能带额外字段，
                // 忽略它们比拒绝整条规则更友好
                _ => {}
            }
        }

        let freq = freq.ok_or_else(|| {
            AppError::validation("重复规则缺少 FREQ").with_hint("例如 FREQ=WEEKLY;BYDAY=MO,WE,FR")
        })?;

        // BYSETPOS 需要 BYDAY 提供星期；若同时存在，取第一个星期
        if let Some(sp) = &mut by_setpos {
            if let Some(w) = by_weekday.first() {
                sp.weekday = *w;
            }
        }

        // 校验语义冲突
        if by_setpos.is_some() && !by_monthday.is_empty() {
            return Err(
                AppError::validation("「每月第几个星期 X」与「每月第几天」不能同时设置")
                    .with_hint("请二选一，它们表达的是两种不同的月度重复方式"),
            );
        }
        if let Some(sp) = by_setpos {
            if freq != Freq::Monthly {
                return Err(AppError::validation("「每月第几个星期 X」只能用于每月重复"));
            }
            if sp.nth > 5 {
                return Err(AppError::validation(format!(
                    "BYSETPOS 超出范围：{}",
                    sp.nth
                )));
            }
        }
        if weekdays_only && freq != Freq::Weekly {
            return Err(AppError::validation("「仅工作日」只能用于每周重复")
                .with_hint("如需每月的某些天，请改用具体日期"));
        }

        // 校验 dtstart_local 合法
        parse_local_datetime(dtstart_local)?;

        Ok(Self {
            freq,
            interval,
            by_weekday,
            by_monthday,
            by_setpos,
            by_month,
            weekdays_only,
            end,
            tzid: tzid.to_string(),
            dtstart_local: dtstart_local.to_string(),
            has_start_time,
        })
    }

    /// 规则的可读中文描述（用于界面展示，§5 要求"定义一致的处理策略并展示给用户"）
    pub fn describe(&self) -> String {
        let every = if self.interval > 1 {
            format!("每 {} ", self.interval)
        } else {
            "每".to_string()
        };

        let unit = match self.freq {
            Freq::Daily => "天",
            Freq::Weekly => "周",
            Freq::Monthly => "个月",
            Freq::Yearly => "年",
        };

        let mut s = format!("{every}{unit}");

        if self.weekdays_only {
            s.push_str("的工作日（周一至周五）");
        } else if let Some(sp) = &self.by_setpos {
            // 必须**先于** by_weekday 判断：解析 BYSETPOS 规则时会同时填充
            // by_weekday（BYDAY 里的星期），若先匹配 by_weekday 就会只说
            // "每周五"，丢掉"第 3 个"这一关键信息。
            let wname = ["周一", "周二", "周三", "周四", "周五", "周六", "周日"]
                .get((sp.weekday as usize).saturating_sub(1))
                .copied()
                .unwrap_or("某天");
            // 中文里"第3个"不插空格更自然，与"每3天"的处理保持一致
            let nth = if sp.nth == -1 {
                "最后".to_string()
            } else {
                format!("第{}", sp.nth)
            };
            s.push_str(&format!("的{nth}个{wname}"));
        } else if !self.by_weekday.is_empty() {
            let names: Vec<&str> = self
                .by_weekday
                .iter()
                .filter_map(|w| {
                    ["周一", "周二", "周三", "周四", "周五", "周六", "周日"]
                        .get((*w as usize).saturating_sub(1))
                        .copied()
                })
                .collect();
            s.push_str(&format!("的{}", names.join("、")));
        } else if !self.by_monthday.is_empty() {
            // 负数要读成"最后一天"而不是"-1 日"，否则用户看到的描述没法理解。
            // 顺带把单位放在正确位置：正数读作"每月 15 日"，
            // 负数读作"每月最后一天"，不该拼成"每月最后一天"。
            let days: Vec<String> = self
                .by_monthday
                .iter()
                .map(|d| match *d {
                    -1 => "最后一天".to_string(),
                    n if n < 0 => format!("倒数第 {} 天", -(n as i32)),
                    n => format!("{n}日"),
                })
                .collect();
            s.push_str(&format!("的{}", days.join("、")));
        } else if !self.by_month.is_empty() {
            let months: Vec<String> = self.by_month.iter().map(|m| m.to_string()).collect();
            s.push_str(&format!("的{}月", months.join("、")));
        }

        match &self.end {
            EndCondition::Never => {}
            EndCondition::Until { date } => s.push_str(&format!("，直到 {date}")),
            EndCondition::Count { count } => s.push_str(&format!("，共 {count} 次")),
        }

        s
    }

    /// 这条规则涉及的边界策略说明（界面必须展示，§5 明确要求）
    pub fn edge_policy_note(&self) -> Option<String> {
        // 负数（-1 = 最后一天）同样属于"触及月末"的策略，必须给用户说明
        let touches_month_end = self.by_monthday.iter().any(|d| *d < 0 || *d > 28)
            || self
                .by_setpos
                .as_ref()
                .map(|s| s.nth == 5 || s.nth == -1)
                .unwrap_or(false)
            || self.freq == Freq::Monthly;

        if !touches_month_end {
            return None;
        }

        Some(
            "当某个重复日在该月不存在时（例如每月 31 日遇到只有 30 天的月份，\
             或 2 月没有 29/30/31 日），**该月跳过这一次**，不会挪到月底。\
             闰年的 2 月 29 日会正常发生。"
                .to_string(),
        )
    }
}

// =============================================================================
// 日期工具
// =============================================================================

/// 解析 `YYYY-MM-DD` 为 NaiveDate
pub fn parse_local_date(s: &str) -> AppResult<chrono::NaiveDate> {
    chrono::NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").map_err(|_| {
        AppError::validation(format!("日期格式不正确：{s}"))
            .with_hint("应为 YYYY-MM-DD，例如 2026-09-30")
    })
}

/// 解析 `YYYY-MM-DDTHH:MM:SS` 为 NaiveDateTime。
///
/// 刻意同时接受空格分隔的 `YYYY-MM-DD HH:MM:SS`：
/// chrono 的 `NaiveDateTime::to_string()` 产出的正是带空格的形式，
/// 而内部几处会把 `Occurrence.local` 直接 `to_string()` 后再解析回来。
/// 只认 `T` 会让这条内部往返路径失败（表现为"日期格式不正确"），
/// 而这与用户输入无关，属于实现细节不该互相打架的地方。
pub fn parse_local_datetime(s: &str) -> AppResult<chrono::NaiveDateTime> {
    let t = s.trim();
    for fmt in [
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
    ] {
        if let Ok(d) = chrono::NaiveDateTime::parse_from_str(t, fmt) {
            return Ok(d);
        }
    }
    Err(AppError::validation(format!("日期时间格式不正确：{s}"))
        .with_hint("应为 YYYY-MM-DDTHH:MM:SS，例如 2026-09-21T09:00:00"))
}

/// 该年该月有多少天
fn days_in_month(year: i32, month: u32) -> u32 {
    let (ny, nm) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    let first_next = chrono::NaiveDate::from_ymd_opt(ny, nm, 1).expect("构造下月首日");
    let first_this = chrono::NaiveDate::from_ymd_opt(year, month, 1).expect("构造本月首日");
    (first_next - first_this).num_days() as u32
}

/// 判断某年是否闰年（格里高利规则）。
///
/// 仅用于测试与断言：展开逻辑通过 chrono 的日期运算天然处理闰年，
/// 不需要自己判断天数，因此标注 cfg(test) 避免生产构建出现死代码。
#[cfg(test)]
fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// 星期编号（1=周一 … 7=周日）
fn iso_weekday(d: chrono::NaiveDate) -> u8 {
    use chrono::Datelike;
    d.weekday().number_from_monday() as u8
}

// =============================================================================
// 展开（惰性生成发生序列）
// =============================================================================

/// 向前探测的最大天数（约 40 年）。
///
/// 作用有两个：
/// 1. 防止规则实际无法产生任何日期时的死循环（例如"每年 2 月 30 日"）；
/// 2. 给周/月/年步进提供一个必要的搜索边界，否则无法知道"本周内是否还有匹配日"。
///
/// 40 年足够涵盖用户可能关心的任何范围（含闰年周期），又不会让单次调用耗时过长。
const MAX_PROBE_DAYS: i64 = 366 * 40;

/// 单次展开允许返回的最大发生次数，防止调用方误传超大 limit
const MAX_OCCURRENCES: usize = 1000;

/// 一次发生
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Occurrence {
    /// 本地墙上时间
    pub local: chrono::NaiveDateTime,
    /// 该次在系列中的序号（从 1 开始）——用于"第 N 次"的稳定身份
    pub index: i64,
}

impl RecurrenceRule {
    /// 判断某个本地日期是否满足本规则的"日期部分"（不考虑 DTSTART 之前的过滤）
    fn date_matches(&self, d: chrono::NaiveDate, anchor: chrono::NaiveDate) -> bool {
        use chrono::Datelike;

        // 频率相关的粗筛
        let period_ok = match self.freq {
            Freq::Daily => {
                // 每天都算一个周期，所以按天数取模。
                // 先前这里直接返回 true，导致 INTERVAL=3 完全失效（每 3 天变成每天），
                // 由 expands_daily_with_interval 测试发现。
                let days = (d - anchor).num_days();
                days >= 0 && days % self.interval == 0
            }

            Freq::Weekly => {
                // 计算 d 与 anchor 之间相差多少个自然周（周一为周首）
                let wd_d = (iso_weekday(d) - 1) as i64;
                let wd_a = (iso_weekday(anchor) - 1) as i64;
                let week_d = d - chrono::Duration::days(wd_d);
                let week_a = anchor - chrono::Duration::days(wd_a);
                let diff_weeks = (week_d - week_a).num_days() / 7;
                diff_weeks >= 0 && diff_weeks % self.interval == 0
            }

            Freq::Monthly => {
                let months = (d.year() as i64 - anchor.year() as i64) * 12
                    + (d.month() as i64 - anchor.month() as i64);
                months >= 0 && months % self.interval == 0
            }

            Freq::Yearly => {
                let years = d.year() as i64 - anchor.year() as i64;
                years >= 0 && years % self.interval == 0
            }
        };
        if !period_ok {
            return false;
        }

        // 年规则的月份过滤
        if self.freq == Freq::Yearly
            && !self.by_month.is_empty()
            && !self.by_month.contains(&(d.month() as u8))
        {
            return false;
        }

        // 每月第 N 个星期 X
        if let Some(sp) = self.by_setpos {
            if iso_weekday(d) != sp.weekday {
                return false;
            }
            let nth = nth_weekday_of_month(d);
            let want = if sp.nth == -1 {
                last_nth_weekday_of_month(d.year(), d.month(), sp.weekday)
            } else {
                sp.nth as u8
            };
            if nth != want {
                return false;
            }
            return true;
        }

        // 每月指定日
        if !self.by_monthday.is_empty() {
            // 负数从月末倒数：-1 = 当月最后一天（RFC 5545）。
            // 按**当月实际天数**解析，因此 2 月的 -1 自动落在 28/29 日，
            // 4 月落在 30 日，不需要为每个月份写特例。
            let day = d.day() as i8;
            let month_len = days_in_month(d.year(), d.month()) as i8;
            let matched = self.by_monthday.iter().any(|want| {
                if *want > 0 {
                    *want == day
                } else {
                    month_len + *want + 1 == day
                }
            });
            return matched;
        }

        // 星期过滤：工作日的简写优先
        if self.weekdays_only {
            return (1..=5).contains(&iso_weekday(d));
        }
        if !self.by_weekday.is_empty() {
            return self.by_weekday.contains(&iso_weekday(d));
        }

        // 没有指定星期时，沿用起始日的星期（WEEKLY）或起始日的日（MONTHLY/YEARLY）
        match self.freq {
            Freq::Weekly => iso_weekday(d) == iso_weekday(anchor),
            Freq::Monthly => d.day() == anchor.day(),
            Freq::Yearly => d.month() == anchor.month() && d.day() == anchor.day(),
            // DAILY 的间隔已在周期粗筛中处理，这里不再过滤
            Freq::Daily => true,
        }
    }

    /// 展开接下来的 `limit` 次发生。
    ///
    /// 从 `dtstart_local` 开始（含起始日），按本地墙上时间推进。
    /// 返回的发生都带有 `index`（从 1 起），可据此构造稳定身份。
    pub fn expand(&self, limit: usize) -> AppResult<Vec<Occurrence>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let limit = limit.min(MAX_OCCURRENCES);

        let start = parse_local_datetime(&self.dtstart_local)?;
        let anchor_date = start.date();

        // UNTIL 的上界（本地日期，含当日）
        let until_date = match &self.end {
            EndCondition::Until { date } => Some(parse_local_date(date)?),
            _ => None,
        };
        // 仅日期任务的发生时刻统一为 00:00:00（已在 dtstart 中体现）
        let time = start.time();

        let mut out: Vec<Occurrence> = Vec::with_capacity(limit);
        let mut index: i64 = 0;
        let mut probed: i64 = 0;
        let mut d = anchor_date;

        while out.len() < limit && probed < MAX_PROBE_DAYS {
            if let Some(ud) = until_date {
                if d > ud {
                    break;
                }
            }

            if self.date_matches(d, anchor_date) {
                index += 1;

                // COUNT 是"总次数上限"，包含起始那次
                if let EndCondition::Count { count } = self.end {
                    if index > count {
                        break;
                    }
                }

                let dt = d.and_time(time);
                out.push(Occurrence { local: dt, index });
            }

            d += chrono::Duration::days(1);
            probed += 1;
        }

        Ok(out)
    }

    /// Expand a requested local range without imposing a lifetime occurrence cap.
    /// The index counts earlier matches so COUNT and occurrence_index keep their
    /// original meaning even when the series is many years old.
    pub fn expand_between(
        &self,
        range_start: chrono::NaiveDateTime,
        range_end: chrono::NaiveDateTime,
        max_results: usize,
    ) -> AppResult<Vec<Occurrence>> {
        if range_end <= range_start || max_results == 0 {
            return Ok(Vec::new());
        }
        let start = parse_local_datetime(&self.dtstart_local)?;
        let anchor = start.date();
        let first = range_start.date().max(anchor);
        let last = range_end.date();
        let until = match &self.end {
            EndCondition::Until { date } => Some(parse_local_date(date)?),
            _ => None,
        };

        // Counting previous matches is independent of the result cap. The scan
        // uses constant memory; only the requested range can fill the output.
        let mut index = 0i64;
        let mut date = anchor;
        while date < first {
            if self.date_matches(date, anchor) {
                index += 1;
            }
            date += chrono::Duration::days(1);
        }

        let mut out = Vec::new();
        date = first;
        while date <= last && out.len() < max_results {
            if until.is_some_and(|bound| date > bound) {
                break;
            }
            if self.date_matches(date, anchor) {
                index += 1;
                if let EndCondition::Count { count } = self.end {
                    if index > count {
                        break;
                    }
                }
                let local = date.and_time(start.time());
                if local >= range_start && local < range_end {
                    out.push(Occurrence { local, index });
                }
            }
            date += chrono::Duration::days(1);
        }
        Ok(out)
    }
}

/// d 是它所在月份的第几个同名星期（1–5）
fn nth_weekday_of_month(d: chrono::NaiveDate) -> u8 {
    use chrono::Datelike;
    ((d.day() - 1) / 7 + 1) as u8
}

/// 某年某月最后一个星期 `weekday` 是第几个（1–5）
fn last_nth_weekday_of_month(year: i32, month: u32, weekday: u8) -> u8 {
    let total = days_in_month(year, month);
    // 从月末往回找第一个匹配的星期
    for day in (1..=total).rev() {
        if let Some(d) = chrono::NaiveDate::from_ymd_opt(year, month, day) {
            if iso_weekday(d) == weekday {
                return ((day - 1) / 7 + 1) as u8;
            }
        }
    }
    0
}

/// 把一批发生转换为 UTC 时刻（供实例落库与提醒使用）。
///
/// 时区解析失败时回退到 UTC，并记录告警——不因为一个陌生的 tzid 让整个
/// 重复任务不可用。
pub fn occurrences_to_utc(
    occ: &[Occurrence],
    tzid: &str,
) -> Vec<(String, chrono::DateTime<chrono::Utc>)> {
    use chrono::TimeZone;

    let tz: Option<chrono_tz::Tz> = tzid.parse().ok();
    if tz.is_none() {
        log::warn!("未知时区标识「{tzid}」，按 UTC 处理");
    }

    occ.iter()
        .map(|o| {
            let utc = match tz {
                Some(tz) => match tz.from_local_datetime(&o.local) {
                    // 夏令时切换可能出现本地时间不存在或重复：
                    // 取最早的那个（earliest），保证行为确定
                    chrono::LocalResult::Single(dt) => dt.with_timezone(&chrono::Utc),
                    chrono::LocalResult::Ambiguous(a, _b) => a.with_timezone(&chrono::Utc),
                    chrono::LocalResult::None => {
                        // 该本地时刻不存在（春季跳变），用 UTC 解释以免丢实例
                        chrono::Utc.from_utc_datetime(&o.local)
                    }
                },
                None => chrono::Utc.from_utc_datetime(&o.local),
            };
            (o.local.to_string(), utc)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_daily_series_expands_requested_range() {
        let rule =
            RecurrenceRule::from_rrule_string("FREQ=DAILY", "UTC", "2018-01-01T09:00:00", true)
                .unwrap();
        let start = parse_local_datetime("2026-04-01T00:00:00").unwrap();
        let end = parse_local_datetime("2026-04-04T00:00:00").unwrap();
        let rows = rule.expand_between(start, end, 500).unwrap();
        assert_eq!(rows.len(), 3);
        assert!(rows[0].index > 3000);
        assert_eq!(rows[0].local.date().to_string(), "2026-04-01");
    }

    fn rule(rrule: &str) -> RecurrenceRule {
        RecurrenceRule::from_rrule_string(rrule, "Asia/Shanghai", "2026-09-21T09:00:00", true)
            .expect("规则应能解析")
    }

    // ------------------------- 解析与序列化 -------------------------

    #[test]
    fn parses_weekly_with_multiple_days() {
        let r = rule("FREQ=WEEKLY;BYDAY=MO,WE,FR");
        assert_eq!(r.freq, Freq::Weekly);
        assert_eq!(r.by_weekday, vec![1, 3, 5]);
        assert_eq!(r.interval, 1);
        assert_eq!(r.end, EndCondition::Never);
    }

    #[test]
    fn parses_interval() {
        let r = rule("FREQ=DAILY;INTERVAL=3");
        assert_eq!(r.freq, Freq::Daily);
        assert_eq!(r.interval, 3);
    }

    #[test]
    fn parses_weekdays_only_shorthand() {
        let r = rule("FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR");
        assert!(r.weekdays_only, "周一到周五应被识别为「仅工作日」");
        assert!(r.by_weekday.is_empty(), "简化形式不应填充 by_weekday");
    }

    #[test]
    fn parses_monthly_by_monthday() {
        let r = rule("FREQ=MONTHLY;BYMONTHDAY=1,15");
        assert_eq!(r.by_monthday, vec![1, 15]);
    }

    #[test]
    fn parses_monthly_by_setpos() {
        let r = rule("FREQ=MONTHLY;BYDAY=FR;BYSETPOS=3");
        let sp = r.by_setpos.expect("应解析出 BYSETPOS");
        assert_eq!(sp.nth, 3);
        assert_eq!(sp.weekday, 5, "星期五");
    }

    #[test]
    fn parses_last_weekday_of_month() {
        let r = rule("FREQ=MONTHLY;BYDAY=FR;BYSETPOS=-1");
        assert_eq!(r.by_setpos.unwrap().nth, -1);
    }

    #[test]
    fn parses_until_and_count() {
        let a = rule("FREQ=DAILY;UNTIL=20261231");
        assert_eq!(
            a.end,
            EndCondition::Until {
                date: "2026-12-31".into()
            }
        );

        let b = rule("FREQ=DAILY;COUNT=10");
        assert_eq!(b.end, EndCondition::Count { count: 10 });
    }

    #[test]
    fn parses_yearly_by_month() {
        let r = rule("FREQ=YEARLY;BYMONTH=3,9");
        assert_eq!(r.by_month, vec![3, 9]);
    }

    #[test]
    fn roundtrip_through_rrule_string() {
        for src in [
            "FREQ=DAILY",
            "FREQ=DAILY;INTERVAL=3",
            "FREQ=WEEKLY;BYDAY=MO,WE,FR",
            "FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR",
            "FREQ=MONTHLY;BYMONTHDAY=1,15,31",
            "FREQ=MONTHLY;BYDAY=FR;BYSETPOS=3",
            "FREQ=YEARLY;BYMONTH=3,9",
            "FREQ=DAILY;COUNT=10",
            "FREQ=DAILY;UNTIL=20261231",
        ] {
            let a = rule(src);
            let s = a.to_rrule_string().unwrap();
            let b =
                RecurrenceRule::from_rrule_string(&s, "Asia/Shanghai", "2026-09-21T09:00:00", true)
                    .unwrap_or_else(|e| panic!("{src} 序列化为 {s} 后应能重新解析：{e}"));
            assert_eq!(a, b, "{src} 往返后应等价（中间形态 {s}）");
        }
    }

    // ------------------------- 非法输入 -------------------------

    #[test]
    fn rejects_missing_freq() {
        let e = RecurrenceRule::from_rrule_string("INTERVAL=2", "UTC", "2026-01-01T00:00:00", true);
        assert!(e.is_err());
        assert!(e.unwrap_err().message.contains("FREQ"));
    }

    #[test]
    fn rejects_unknown_freq() {
        assert!(RecurrenceRule::from_rrule_string(
            "FREQ=HOURLY",
            "UTC",
            "2026-01-01T00:00:00",
            true
        )
        .is_err());
    }

    #[test]
    fn rejects_invalid_interval_and_count() {
        assert!(RecurrenceRule::from_rrule_string(
            "FREQ=DAILY;INTERVAL=0",
            "UTC",
            "2026-01-01T00:00:00",
            true
        )
        .is_err());
        assert!(RecurrenceRule::from_rrule_string(
            "FREQ=DAILY;COUNT=0",
            "UTC",
            "2026-01-01T00:00:00",
            true
        )
        .is_err());
    }

    #[test]
    fn rejects_out_of_range_monthday_and_month() {
        assert!(RecurrenceRule::from_rrule_string(
            "FREQ=MONTHLY;BYMONTHDAY=32",
            "UTC",
            "2026-01-01T00:00:00",
            true
        )
        .is_err());
        assert!(RecurrenceRule::from_rrule_string(
            "FREQ=YEARLY;BYMONTH=13",
            "UTC",
            "2026-01-01T00:00:00",
            true
        )
        .is_err());
    }

    /// 「每月第 N 个星期 X」与「每月第几天」是两种不同的月度语义，
    /// 同时给会让用户无法预期结果，必须拒绝。
    #[test]
    fn rejects_conflicting_monthly_specs() {
        let e = RecurrenceRule::from_rrule_string(
            "FREQ=MONTHLY;BYDAY=FR;BYSETPOS=3;BYMONTHDAY=15",
            "UTC",
            "2026-01-01T00:00:00",
            true,
        );
        assert!(e.is_err());
        assert!(e.unwrap_err().message.contains("不能同时设置"));
    }

    #[test]
    fn rejects_setpos_outside_monthly() {
        assert!(RecurrenceRule::from_rrule_string(
            "FREQ=WEEKLY;BYDAY=FR;BYSETPOS=3",
            "UTC",
            "2026-01-01T00:00:00",
            true
        )
        .is_err());
    }

    #[test]
    fn rejects_weekdays_only_outside_weekly() {
        assert!(RecurrenceRule::from_rrule_string(
            "FREQ=DAILY;BYDAY=MO,TU,WE,TH,FR",
            "UTC",
            "2026-01-01T00:00:00",
            true
        )
        .is_err());
    }

    #[test]
    fn rejects_invalid_dtstart() {
        assert!(RecurrenceRule::from_rrule_string(
            "FREQ=DAILY",
            "UTC",
            "2026-13-45T00:00:00",
            true
        )
        .is_err());
        assert!(
            RecurrenceRule::from_rrule_string("FREQ=DAILY", "UTC", "not-a-date", true).is_err()
        );
        // 只有日期没有时刻也不行——推进运算需要完整时间
        assert!(
            RecurrenceRule::from_rrule_string("FREQ=DAILY", "UTC", "2026-01-01", true).is_err()
        );
    }

    /// 未知键应被忽略而不是报错：不同工具生成的 RRULE 常带额外字段，
    /// 拒绝整条规则会让用户无法导入。
    #[test]
    fn ignores_unknown_rrule_keys() {
        let r = rule("FREQ=DAILY;WKST=SU;X-UNKNOWN=1");
        assert_eq!(r.freq, Freq::Daily);
    }

    // ------------------------- 描述文案 -------------------------

    #[test]
    fn describes_weekly_rule_in_chinese() {
        let s = rule("FREQ=WEEKLY;BYDAY=MO,WE,FR").describe();
        assert!(s.contains("周"), "应包含周期单位：{s}");
        assert!(
            s.contains("周一") && s.contains("周三") && s.contains("周五"),
            "{s}"
        );
    }

    #[test]
    fn describes_interval_and_end_condition() {
        let s = rule("FREQ=DAILY;INTERVAL=3;COUNT=10").describe();
        assert!(s.contains("每 3 天"), "{s}");
        assert!(s.contains("共 10 次"), "{s}");

        let s2 = rule("FREQ=DAILY;UNTIL=20261231").describe();
        assert!(s2.contains("直到 2026-12-31"), "{s2}");
    }

    #[test]
    fn describes_setpos_rule() {
        let s = rule("FREQ=MONTHLY;BYDAY=FR;BYSETPOS=3").describe();
        assert!(s.contains("第3个"), "{s}");
        assert!(s.contains("周五"), "{s}");
        // 不应出现多余空白（中文排版里"第 3个"很难看）
        assert!(!s.contains("第 "), "{s}");

        let last = rule("FREQ=MONTHLY;BYDAY=FR;BYSETPOS=-1").describe();
        assert!(last.contains("最后"), "{last}");
        assert!(last.contains("周五"), "{last}");
    }

    /// §5 明确要求把边界策略展示给用户，因此涉及月末的规则必须给出说明。
    #[test]
    fn edge_policy_note_present_for_month_end_rules() {
        assert!(rule("FREQ=MONTHLY;BYMONTHDAY=31")
            .edge_policy_note()
            .is_some());
        assert!(rule("FREQ=MONTHLY;BYMONTHDAY=1")
            .edge_policy_note()
            .is_some());
        assert!(rule("FREQ=MONTHLY;BYDAY=FR;BYSETPOS=-1")
            .edge_policy_note()
            .is_some());
        // 每日规则与月末无关，不该打扰用户
        assert!(rule("FREQ=DAILY").edge_policy_note().is_none());
        assert!(rule("FREQ=WEEKLY;BYDAY=MO").edge_policy_note().is_none());
    }

    // ------------------------- 日期工具 -------------------------

    #[test]
    fn days_in_month_handles_leap_years() {
        assert_eq!(days_in_month(2026, 2), 28, "2026 是平年");
        assert_eq!(days_in_month(2024, 2), 29, "2024 是闰年");
        assert_eq!(days_in_month(2000, 2), 29, "2000 是闰年（能被 400 整除）");
        assert_eq!(
            days_in_month(1900, 2),
            28,
            "1900 不是闰年（能被 100 但不能被 400 整除）"
        );
        assert_eq!(days_in_month(2026, 1), 31);
        assert_eq!(days_in_month(2026, 4), 30);
        assert_eq!(days_in_month(2026, 12), 31);
    }

    #[test]
    fn leap_year_detection_matches_gregorian_rules() {
        assert!(is_leap(2024));
        assert!(!is_leap(2026));
        assert!(is_leap(2000));
        assert!(!is_leap(1900));
        assert!(!is_leap(2100));
    }

    #[test]
    fn iso_weekday_maps_monday_to_one() {
        use chrono::NaiveDate;
        // 2026-09-21 是周一
        assert_eq!(
            iso_weekday(NaiveDate::from_ymd_opt(2026, 9, 21).unwrap()),
            1
        );
        // 2026-09-27 是周日
        assert_eq!(
            iso_weekday(NaiveDate::from_ymd_opt(2026, 9, 27).unwrap()),
            7
        );
    }

    // =========================================================================
    // 展开（§5 验收例：创建周一/三/五重复任务）
    // =========================================================================

    /// 每周一三五：从 2026-09-21（周一）起，接下来几次应是 21/23/25/28/30…
    #[test]
    fn expands_weekly_mon_wed_fri() {
        let r = RecurrenceRule::from_rrule_string(
            "FREQ=WEEKLY;BYDAY=MO,WE,FR",
            "Asia/Shanghai",
            "2026-09-21T09:00:00",
            true,
        )
        .unwrap();

        let occ = r.expand(6).unwrap();
        let dates: Vec<String> = occ
            .iter()
            .map(|o| o.local.date().format("%Y-%m-%d").to_string())
            .collect();
        assert_eq!(
            dates,
            vec![
                "2026-09-21", // 周一
                "2026-09-23", // 周三
                "2026-09-25", // 周五
                "2026-09-28", // 周一
                "2026-09-30", // 周三
                "2026-10-02", // 周五
            ]
        );
        // 序号必须连续，它是"第几次"稳定身份的基础
        assert_eq!(
            occ.iter().map(|o| o.index).collect::<Vec<_>>(),
            vec![1, 2, 3, 4, 5, 6]
        );
    }

    /// 所有发生必须保留起始时刻（09:00），不会因为日期推进而漂移
    #[test]
    fn expansion_preserves_time_of_day() {
        let r = RecurrenceRule::from_rrule_string(
            "FREQ=DAILY",
            "Asia/Shanghai",
            "2026-09-21T09:30:00",
            true,
        )
        .unwrap();
        for o in r.expand(10).unwrap() {
            assert_eq!(o.local.time().to_string(), "09:30:00");
        }
    }

    #[test]
    fn expands_daily_with_interval() {
        let r = RecurrenceRule::from_rrule_string(
            "FREQ=DAILY;INTERVAL=3",
            "Asia/Shanghai",
            "2026-09-21T08:00:00",
            true,
        )
        .unwrap();
        let dates: Vec<String> = r
            .expand(4)
            .unwrap()
            .iter()
            .map(|o| o.local.date().format("%m-%d").to_string())
            .collect();
        assert_eq!(dates, vec!["09-21", "09-24", "09-27", "09-30"]);
    }

    #[test]
    fn expands_weekdays_only() {
        // 从周五 2026-09-25 起，工作日序列应跳过周末
        let r = RecurrenceRule::from_rrule_string(
            "FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR",
            "Asia/Shanghai",
            "2026-09-25T09:00:00",
            true,
        )
        .unwrap();
        let dates: Vec<String> = r
            .expand(4)
            .unwrap()
            .iter()
            .map(|o| o.local.date().format("%Y-%m-%d").to_string())
            .collect();
        assert_eq!(
            dates,
            vec!["2026-09-25", "2026-09-28", "2026-09-29", "2026-09-30"],
            "周五之后应是下周一，跳过 26/27 两天周末"
        );
    }

    /// COUNT 是总次数上限（含起始那次），达到后必须停止
    #[test]
    fn count_limits_total_occurrences() {
        let r = RecurrenceRule::from_rrule_string(
            "FREQ=DAILY;COUNT=3",
            "Asia/Shanghai",
            "2026-09-21T09:00:00",
            true,
        )
        .unwrap();
        let occ = r.expand(10).unwrap();
        assert_eq!(occ.len(), 3, "COUNT=3 应只产生 3 次");
        assert_eq!(
            occ.last().unwrap().local.date().format("%m-%d").to_string(),
            "09-23"
        );
    }

    /// UNTIL 的日期是含当日的
    #[test]
    fn until_is_inclusive() {
        let r = RecurrenceRule::from_rrule_string(
            "FREQ=DAILY;UNTIL=20260923",
            "Asia/Shanghai",
            "2026-09-21T09:00:00",
            true,
        )
        .unwrap();
        let dates: Vec<String> = r
            .expand(10)
            .unwrap()
            .iter()
            .map(|o| o.local.date().format("%m-%d").to_string())
            .collect();
        assert_eq!(dates, vec!["09-21", "09-22", "09-23"], "应包含 UNTIL 当天");
    }

    // ------------------------- 月末与闰年（§5 明确要求） -------------------------

    /// 每月 31 日：只有 31 天的月份发生，其余月份**跳过**（不挪到 30 日）
    #[test]
    fn monthly_31st_skips_short_months() {
        let r = RecurrenceRule::from_rrule_string(
            "FREQ=MONTHLY;BYMONTHDAY=31",
            "Asia/Shanghai",
            "2026-01-31T10:00:00",
            true,
        )
        .unwrap();

        let dates: Vec<String> = r
            .expand(5)
            .unwrap()
            .iter()
            .map(|o| o.local.date().format("%Y-%m-%d").to_string())
            .collect();
        // 2026 年中 1/3/5/7/8/10/12 月有 31 天；2 月只有 28 天被跳过
        assert_eq!(
            dates,
            vec![
                "2026-01-31",
                "2026-03-31", // 2 月被跳过
                "2026-05-31", // 4 月被跳过
                "2026-07-31", // 6 月被跳过
                "2026-08-31",
            ],
            "每月 31 日不应被挪到 30 日或月末"
        );
    }

    /// 每月 29 日：2026 年 2 月只有 28 天 → 跳过；2028 年闰年 2 月应有 29 日
    #[test]
    fn monthly_29th_handles_leap_year_february() {
        let r = RecurrenceRule::from_rrule_string(
            "FREQ=MONTHLY;BYMONTHDAY=29",
            "Asia/Shanghai",
            "2026-01-29T10:00:00",
            true,
        )
        .unwrap();

        let dates: Vec<String> = r
            .expand(4)
            .unwrap()
            .iter()
            .map(|o| o.local.date().format("%Y-%m-%d").to_string())
            .collect();
        assert_eq!(
            dates,
            vec!["2026-01-29", "2026-03-29", "2026-04-29", "2026-05-29"],
            "2026 年 2 月没有 29 日，应跳过"
        );

        // 跨到 2028 年 2 月：闰年应有 29 日
        let r2 = RecurrenceRule::from_rrule_string(
            "FREQ=MONTHLY;BYMONTHDAY=29",
            "Asia/Shanghai",
            "2028-01-29T10:00:00",
            true,
        )
        .unwrap();
        let d2: Vec<String> = r2
            .expand(2)
            .unwrap()
            .iter()
            .map(|o| o.local.date().format("%Y-%m-%d").to_string())
            .collect();
        assert_eq!(
            d2,
            vec!["2028-01-29", "2028-02-29"],
            "闰年 2 月 29 日应正常发生"
        );
    }

    /// 每年 2 月 29 日：只在闰年发生，平年跳过
    #[test]
    fn yearly_feb_29_only_in_leap_years() {
        let r = RecurrenceRule::from_rrule_string(
            "FREQ=YEARLY;BYMONTH=2;BYMONTHDAY=29",
            "Asia/Shanghai",
            "2024-02-29T10:00:00",
            true,
        )
        .unwrap();

        let dates: Vec<String> = r
            .expand(3)
            .unwrap()
            .iter()
            .map(|o| o.local.date().format("%Y-%m-%d").to_string())
            .collect();
        assert_eq!(
            dates,
            vec!["2024-02-29", "2028-02-29", "2032-02-29"],
            "下一个闰年是 2028（2026/2027 平年应跳过）"
        );
    }

    /// 跨年：从 12 月推进到下一年 1 月必须正确
    #[test]
    fn expansion_crosses_year_boundary() {
        let r = RecurrenceRule::from_rrule_string(
            "FREQ=DAILY",
            "Asia/Shanghai",
            "2026-12-30T09:00:00",
            true,
        )
        .unwrap();
        let dates: Vec<String> = r
            .expand(4)
            .unwrap()
            .iter()
            .map(|o| o.local.date().format("%Y-%m-%d").to_string())
            .collect();
        assert_eq!(
            dates,
            vec!["2026-12-30", "2026-12-31", "2027-01-01", "2027-01-02"]
        );
    }

    /// 每月第 3 个星期五
    #[test]
    fn expands_third_friday_of_month() {
        let r = RecurrenceRule::from_rrule_string(
            "FREQ=MONTHLY;BYDAY=FR;BYSETPOS=3",
            "Asia/Shanghai",
            "2026-09-18T15:00:00",
            true,
        )
        .unwrap();
        let dates: Vec<String> = r
            .expand(3)
            .unwrap()
            .iter()
            .map(|o| o.local.date().format("%Y-%m-%d").to_string())
            .collect();
        // 2026-09-18 是 9 月第 3 个周五；10 月第 3 个周五是 10-16；11 月是 11-20
        assert_eq!(dates, vec!["2026-09-18", "2026-10-16", "2026-11-20"]);
    }

    /// 每月最后一个星期五
    #[test]
    fn expands_last_friday_of_month() {
        let r = RecurrenceRule::from_rrule_string(
            "FREQ=MONTHLY;BYDAY=FR;BYSETPOS=-1",
            "Asia/Shanghai",
            "2026-09-25T15:00:00",
            true,
        )
        .unwrap();
        let dates: Vec<String> = r
            .expand(3)
            .unwrap()
            .iter()
            .map(|o| o.local.date().format("%Y-%m-%d").to_string())
            .collect();
        // 2026-09-25 是 9 月最后一个周五；10-30；11-27
        assert_eq!(dates, vec!["2026-09-25", "2026-10-30", "2026-11-27"]);
    }

    /// 每 2 周
    #[test]
    fn expands_biweekly() {
        let r = RecurrenceRule::from_rrule_string(
            "FREQ=WEEKLY;INTERVAL=2;BYDAY=MO",
            "Asia/Shanghai",
            "2026-09-21T09:00:00",
            true,
        )
        .unwrap();
        let dates: Vec<String> = r
            .expand(3)
            .unwrap()
            .iter()
            .map(|o| o.local.date().format("%m-%d").to_string())
            .collect();
        assert_eq!(dates, vec!["09-21", "10-05", "10-19"], "应隔一周一次");
    }

    /// 每 3 个月
    #[test]
    fn expands_quarterly() {
        let r = RecurrenceRule::from_rrule_string(
            "FREQ=MONTHLY;INTERVAL=3;BYMONTHDAY=15",
            "Asia/Shanghai",
            "2026-01-15T10:00:00",
            true,
        )
        .unwrap();
        let dates: Vec<String> = r
            .expand(4)
            .unwrap()
            .iter()
            .map(|o| o.local.date().format("%Y-%m").to_string())
            .collect();
        assert_eq!(dates, vec!["2026-01", "2026-04", "2026-07", "2026-10"]);
    }

    /// 每年 3 月和 9 月的 1 日
    #[test]
    fn expands_yearly_two_months() {
        let r = RecurrenceRule::from_rrule_string(
            "FREQ=YEARLY;BYMONTH=3,9;BYMONTHDAY=1",
            "Asia/Shanghai",
            "2026-03-01T10:00:00",
            true,
        )
        .unwrap();
        let dates: Vec<String> = r
            .expand(4)
            .unwrap()
            .iter()
            .map(|o| o.local.date().format("%Y-%m-%d").to_string())
            .collect();
        assert_eq!(
            dates,
            vec!["2026-03-01", "2026-09-01", "2027-03-01", "2027-09-01"]
        );
    }

    /// 仅日期任务的发生时刻必须是 00:00:00，不能被当成凌晨到期的"精确时间"
    #[test]
    fn date_only_occurrences_are_midnight() {
        let r = RecurrenceRule::from_rrule_string(
            "FREQ=DAILY",
            "Asia/Shanghai",
            "2026-09-21T00:00:00",
            false,
        )
        .unwrap();
        for o in r.expand(3).unwrap() {
            assert_eq!(o.local.time().to_string(), "00:00:00");
        }
        assert!(!r.has_start_time);
    }

    /// 永远不可能发生的规则必须安全返回空，而不是死循环。
    /// "每年 2 月 30 日"就是这种输入。
    #[test]
    fn impossible_rule_terminates_with_empty_result() {
        // 直接构造（解析层会拒绝 2 月 30 日这种组合，但展开层也要能自保）
        let r = RecurrenceRule {
            freq: Freq::Yearly,
            interval: 1,
            by_weekday: vec![],
            by_monthday: vec![30],
            by_setpos: None,
            by_month: vec![2],
            weekdays_only: false,
            end: EndCondition::Never,
            tzid: "UTC".into(),
            dtstart_local: "2026-02-01T00:00:00".into(),
            has_start_time: false,
        };
        let occ = r.expand(5).unwrap();
        assert!(occ.is_empty(), "不可能发生的规则应返回空结果而非卡住");
    }

    #[test]
    fn expand_zero_limit_returns_empty() {
        let r = rule("FREQ=DAILY");
        assert!(r.expand(0).unwrap().is_empty());
    }

    /// 展开次数上限必须被强制，防止调用方误传超大值
    #[test]
    fn expand_caps_limit() {
        let r = rule("FREQ=DAILY");
        let occ = r.expand(usize::MAX).unwrap();
        assert!(occ.len() <= MAX_OCCURRENCES);
        assert_eq!(occ.len(), MAX_OCCURRENCES);
    }

    // ------------------------- 时区转换 -------------------------

    /// 北京时间的发生转 UTC 后应减去 8 小时
    #[test]
    fn occurrences_convert_to_utc_with_offset() {
        let r = RecurrenceRule::from_rrule_string(
            "FREQ=DAILY",
            "Asia/Shanghai",
            "2026-09-21T09:00:00",
            true,
        )
        .unwrap();
        let occ = r.expand(1).unwrap();
        let utc = occurrences_to_utc(&occ, "Asia/Shanghai");
        assert_eq!(
            utc[0].1.format("%Y-%m-%dT%H:%M:%S").to_string(),
            "2026-09-21T01:00:00"
        );
    }

    /// 未知时区应回退为 UTC 而不是失败——不能让一个陌生 tzid 让整个任务不可用
    #[test]
    fn unknown_timezone_falls_back_to_utc() {
        let r = RecurrenceRule::from_rrule_string(
            "FREQ=DAILY",
            "Not/AZone",
            "2026-09-21T09:00:00",
            true,
        )
        .unwrap();
        let occ = r.expand(1).unwrap();
        let utc = occurrences_to_utc(&occ, "Not/AZone");
        assert_eq!(
            utc[0].1.format("%H:%M").to_string(),
            "09:00",
            "回退后时刻不变"
        );
    }

    /// 跨夏令时的墙上时刻保持不变（这是选择"本地墙上时间推进"的原因）
    #[test]
    fn wall_clock_holds_across_dst() {
        // 美国东部 2026-11-01 凌晨 2 点回拨到 1 点
        let r = RecurrenceRule::from_rrule_string(
            "FREQ=DAILY",
            "America/New_York",
            "2026-10-30T09:00:00",
            true,
        )
        .unwrap();
        let occ = r.expand(5).unwrap();
        // 每一次的本地墙上时刻都应是 09:00，不因夏令时切换而变成 08:00 或 10:00
        for o in &occ {
            assert_eq!(o.local.time().to_string(), "09:00:00", "墙上时刻必须稳定");
        }
        // 但换算成 UTC 后偏移量会变化（EDT 是 -4，EST 是 -5）
        let utc = occurrences_to_utc(&occ, "America/New_York");
        let offsets: Vec<i32> = utc
            .iter()
            .map(|(_, u)| u.format("%H").to_string().parse().unwrap())
            .collect();
        assert_eq!(offsets[0], 13, "10-30 是 EDT（-4），09:00 → 13:00 UTC");
        assert_eq!(offsets[4], 14, "11-03 已转 EST（-5），09:00 → 14:00 UTC");
    }
}

/// 重复任务专项回归（整改任务书 §14）。
///
/// 这个模块**刻意按任务书清单逐条对应**写，而不是零散补测试：
/// 日 / 周 / 月 / 年四种频率、月末与闰日这种最容易出错的边界、
/// 夏令时时区的墙上时刻语义。改规则引擎时先跑这一组。
#[cfg(test)]
mod regression_task_book_14 {
    use super::*;

    /// 展开成 `MM-DD` 列表，便于断言
    fn expand_dates(rrule: &str, tz: &str, dtstart: &str, n: usize) -> Vec<String> {
        RecurrenceRule::from_rrule_string(rrule, tz, dtstart, true)
            .expect("规则应能解析")
            .expand(n)
            .expect("应能展开")
            .iter()
            .map(|o| o.local.date().format("%m-%d").to_string())
            .collect()
    }

    // ------------------------------ 日重复 ------------------------------

    #[test]
    fn daily_plain() {
        assert_eq!(
            expand_dates("FREQ=DAILY", "Asia/Shanghai", "2026-09-21T08:00:00", 3),
            vec!["09-21", "09-22", "09-23"]
        );
    }

    #[test]
    fn daily_every_two_days() {
        assert_eq!(
            expand_dates(
                "FREQ=DAILY;INTERVAL=2",
                "Asia/Shanghai",
                "2026-09-21T08:00:00",
                4
            ),
            vec!["09-21", "09-23", "09-25", "09-27"]
        );
    }

    // ------------------------------ 周重复 ------------------------------

    #[test]
    fn weekly_monday_only() {
        // 2026-09-21 是周一
        assert_eq!(
            expand_dates(
                "FREQ=WEEKLY;BYDAY=MO",
                "Asia/Shanghai",
                "2026-09-21T09:00:00",
                3
            ),
            vec!["09-21", "09-28", "10-05"]
        );
    }

    #[test]
    fn weekly_mon_wed_fri() {
        assert_eq!(
            expand_dates(
                "FREQ=WEEKLY;BYDAY=MO,WE,FR",
                "Asia/Shanghai",
                "2026-09-21T09:00:00",
                4
            ),
            vec!["09-21", "09-23", "09-25", "09-28"]
        );
    }

    #[test]
    fn weekly_every_two_weeks() {
        assert_eq!(
            expand_dates(
                "FREQ=WEEKLY;INTERVAL=2;BYDAY=MO",
                "Asia/Shanghai",
                "2026-09-21T09:00:00",
                3
            ),
            vec!["09-21", "10-05", "10-19"]
        );
    }

    // ------------------------------ 月重复 ------------------------------

    #[test]
    fn monthly_first_day() {
        assert_eq!(
            expand_dates(
                "FREQ=MONTHLY;BYMONTHDAY=1",
                "Asia/Shanghai",
                "2026-01-01T09:00:00",
                3
            ),
            vec!["01-01", "02-01", "03-01"]
        );
    }

    /// 每月 31 日：只有 31 天的月份才发生，短月**跳过**而不是挪到 30/28 日。
    /// 这是"不可用日期"策略里最容易引起争议的一条，因此用测试固定行为。
    #[test]
    fn monthly_31st_skips_short_months() {
        let dates = expand_dates(
            "FREQ=MONTHLY;BYMONTHDAY=31",
            "Asia/Shanghai",
            "2026-01-31T09:00:00",
            5,
        );
        assert_eq!(
            dates,
            vec!["01-31", "03-31", "05-31", "07-31", "08-31"],
            "2/4/6 月没有 31 日，应跳过而不是顺延"
        );
    }

    /// 每月最后一天：不论大小月都应落在当月最后一天
    #[test]
    fn monthly_last_day_follows_month_length() {
        let dates = expand_dates(
            "FREQ=MONTHLY;BYMONTHDAY=-1",
            "Asia/Shanghai",
            "2026-01-31T09:00:00",
            5,
        );
        assert_eq!(
            dates,
            vec!["01-31", "02-28", "03-31", "04-30", "05-31"],
            "平年 2 月是 28 日"
        );
    }

    // ------------------------------ 年重复 ------------------------------

    #[test]
    fn yearly_fixed_date() {
        assert_eq!(
            expand_dates(
                "FREQ=YEARLY;BYMONTH=9;BYMONTHDAY=21",
                "Asia/Shanghai",
                "2026-09-21T09:00:00",
                3
            ),
            vec!["09-21", "09-21", "09-21"]
        );
    }

    /// 闰日：2 月 29 日只在闰年发生（2028 是闰年，2027 不是）
    #[test]
    fn yearly_feb_29_only_in_leap_years() {
        let rule = RecurrenceRule::from_rrule_string(
            "FREQ=YEARLY;BYMONTH=2;BYMONTHDAY=29",
            "Asia/Shanghai",
            "2027-03-01T09:00:00",
            true,
        )
        .unwrap();
        let occ = rule.expand(3).unwrap();
        let years: Vec<i32> = occ
            .iter()
            .map(|o| {
                use chrono::Datelike;
                o.local.year()
            })
            .collect();
        assert_eq!(years, vec![2028, 2032, 2036], "只应在闰年出现");
        for o in &occ {
            assert_eq!(o.local.date().format("%m-%d").to_string(), "02-29");
        }
    }

    // ---------------------------- DST / 时区 ----------------------------

    /// 本地 09:00 的重复任务在夏令时切换前后都必须保持"当地时间 09:00"，
    /// 而不是固定 UTC（那会导致切换后本地时间漂移到 08:00 或 10:00）。
    #[test]
    fn dst_keeps_local_wall_clock() {
        let rule = RecurrenceRule::from_rrule_string(
            "FREQ=DAILY",
            "America/New_York",
            // 2026-03-08 是美国东部进入夏令时的日子（02:00 → 03:00）
            "2026-03-07T09:00:00",
            true,
        )
        .unwrap();
        let occ = rule.expand(4).unwrap();
        for o in &occ {
            assert_eq!(
                o.local.time().to_string(),
                "09:00:00",
                "跨夏令时切换后，本地墙上时刻必须仍是 09:00"
            );
        }
        let utc = occurrences_to_utc(&occ, "America/New_York");
        // 切换前 EST(-5) → 14:00 UTC；切换后 EDT(-4) → 13:00 UTC
        let hours: Vec<u32> = utc
            .iter()
            .map(|(_, u)| u.format("%H").to_string().parse().unwrap())
            .collect();
        assert_eq!(hours.first(), Some(&14), "03-07 仍是 EST");
        assert_eq!(hours.last(), Some(&13), "03-10 已是 EDT");
    }
}
