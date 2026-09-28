//! 中国大陆年度放假安排（整段放假，包括调休日）。未知年份不推测。
//! 来源与更新步骤见 docs/design-workday-calendar.md。
use chrono::{Datelike, NaiveDate};

pub const LAST_YEAR: i32 = 2026;

type Calendar = (&'static [(u32, u32)], &'static [u32]);
fn calendar(year: i32) -> Option<Calendar> {
    Some(match year {
        2025 => (
            &[
                (101, 101),
                (128, 204),
                (404, 406),
                (501, 505),
                (531, 602),
                (1001, 1008),
            ],
            &[126, 208, 427, 928, 1011],
        ),
        2026 => (
            &[
                (101, 103),
                (215, 223),
                (404, 406),
                (501, 505),
                (619, 621),
                (925, 927),
                (1001, 1007),
            ],
            &[104, 214, 228, 509, 920, 1010],
        ),
        _ => return None,
    })
}

/// Some(false) 为放假日；Some(true) 为普通工作日或调休补班；None 为未知年份。
pub fn is_workday(date: NaiveDate) -> Option<bool> {
    let (holidays, makeup) = calendar(date.year())?;
    let md = date.month() * 100 + date.day();
    if holidays
        .iter()
        .any(|&(start, end)| start <= md && md <= end)
    {
        Some(false)
    } else {
        Some(makeup.contains(&md) || date.weekday().number_from_monday() <= 5)
    }
}

/// 仅判断是否属于整段放假；普通双休日不算此处的节假日。
pub fn is_holiday(date: NaiveDate) -> Option<bool> {
    let (ranges, _) = calendar(date.year())?;
    let md = date.month() * 100 + date.day();
    Some(ranges.iter().any(|&(a, b)| a <= md && md <= b))
}
