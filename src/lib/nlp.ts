/**
 * 自然语言日期解析（§4.4）。
 *
 * 关键约束：**解析结果必须在保存前让用户看到并改正**。
 * 因此本模块只负责"提出候选"，绝不直接写库；UI 必须把命中的片段
 * 高亮展示出来，用户确认后才生效。
 *
 * 解析范围刻意保守：只识别中文里最常见、歧义最小的表达。
 * 识别不了就返回 null，由用户手填——而不是猜一个可能错误的日期。
 */

/** 解析结果 */
export interface ParsedInput {
  /** 去掉日期词后的标题 */
  title: string
  /** 解析到的本地日期时间；null 表示未识别 */
  date: Date | null
  /** 命中的原文片段，用于在 UI 中高亮提示 */
  matched: string | null
  /** 命中规则的可读说明，例如「明天 09:00」 */
  label: string | null
  /** 是否含具体时刻（决定 hasPlannedTime） */
  hasTime: boolean
  /** 识别到的标签名（#标签） */
  tags: string[]
  /** 识别到的优先级（!1..!3 或 ！高） */
  priority: number | null
}

const WEEKDAYS: Record<string, number> = {
  一: 1, 二: 2, 三: 3, 四: 4, 五: 5, 六: 6, 日: 7, 天: 7,
}

/** 把 12 小时制的下午时段换算成 24 小时制 */
function toHour(h: number, isPm: boolean): number {
  if (!isPm) return h
  return h < 12 ? h + 12 : h
}

/**
 * 解析一行快速输入。
 *
 * 支持的形式（示例）：
 * - `明天 10:00 交周报`
 * - `今天下午3点 打电话`
 * - `周五 开会`
 * - `下周一 提交材料`
 * - `9月30日 续费`
 * - `3天后 回访客户`
 * - 组合标签与优先级：`明天 写周报 #工作 !2`
 */
export function parseQuickInput(
  raw: string,
  now: Date = new Date(),
): ParsedInput {
  let text = raw
  let date: Date | null = null
  let matched: string | null = null
  let label: string | null = null
  let hasTime = false

  // ---------------- 1. 标签 #xxx ----------------
  const tags: string[] = []
  text = text.replace(/#([\p{L}\p{N}_-]+)/gu, (_m, name: string) => {
    tags.push(name)
    return ''
  })

  // ---------------- 2. 优先级 !1..!3 ----------------
  let priority: number | null = null
  text = text.replace(/[!！]([123])/g, (_m, n: string) => {
    priority = Number(n)
    return ''
  })

  // ---------------- 3. 时间 HH:MM ----------------
  let hour: number | null = null
  let minute = 0
  const colonMatch = text.match(/(?:^|\s)([01]?\d|2[0-3])[:：]([0-5]\d)(?=\s|$)/)
  if (colonMatch) {
    hour = Number(colonMatch[1])
    minute = Number(colonMatch[2])
    hasTime = true
  } else {
    // `下午3点` / `上午9点半` / `晚上8点`
    const cnMatch = text.match(/(上午|早上|中午|下午|傍晚|晚上)?\s*(\d{1,2}|[一二三四五六七八九十]+)\s*[点時时](半)?/)
    if (cnMatch) {
      const rawHour = cnMatch[2] ?? ''
      const h = /^\d+$/.test(rawHour) ? Number(rawHour) : cnNumberToInt(rawHour)
      if (h !== null && h >= 0 && h <= 24) {
        const period = cnMatch[1] ?? ''
        const isPm = /下午|傍晚|晚上/.test(period)
        hour = toHour(h === 24 ? 0 : h, isPm)
        minute = cnMatch[3] ? 30 : 0
        hasTime = true
      }
    }
  }

  // ---------------- 4. 日期 ----------------
  // 顺序很重要：先识别"周几"，再识别"月日"。
  // 否则「下周一」会被 M月D日 规则误匹配成「周一」→ 1 月 1 日。
  const base = new Date(now.getFullYear(), now.getMonth(), now.getDate())

  const setDate = (d: Date, hit: string, desc: string) => {
    if (date) return // 先命中的优先，避免多个日期词互相覆盖
    date = d
    matched = hit
    label = desc
  }

  // 首先检查是否明确写了日期（"明天"/"9月30日"/"周五"等），
  // 用于后面判断"只写了时间"的情况。
  const hasExplicitDay =
    /今天|今日|明天|明日|后天/.test(text) ||
    /(下+)?(?:周|星期|礼拜)[一二三四五六日天]/.test(text) ||
    /(?:(\d{4})\s*年)?\s*(\d{1,2})\s*月\s*(\d{1,2})\s*[日号]?/.test(text) ||
    /([0-9一二三四五六七八九十两]+)\s*天后/.test(text)

  // 相对天数
  const relMatch = text.match(/([0-9一二三四五六七八九十两]+)\s*天后/)
  if (relMatch) {
    const n = /^\d+$/.test(relMatch[1] ?? '') ? Number(relMatch[1]) : cnNumberToInt(relMatch[1] ?? '')
    if (n !== null) {
      const d = new Date(base)
      d.setDate(d.getDate() + n)
      setDate(d, relMatch[0], `${n} 天后`)
    }
  }

  if (!date) {
    if (/今天|今日/.test(text)) setDate(new Date(base), /今天|今日/.exec(text)![0], '今天')
    else if (/明天|明日/.test(text)) {
      const d = new Date(base)
      d.setDate(d.getDate() + 1)
      setDate(d, /明天|明日/.exec(text)![0], '明天')
    } else if (/后天/.test(text)) {
      const d = new Date(base)
      d.setDate(d.getDate() + 2)
      setDate(d, '后天', '后天')
    }
  }

  // 周几（含"下周三"）
  if (!date) {
    const weekMatch = text.match(/(下+)?(?:周|星期|礼拜)([一二三四五六日天])/)
    if (weekMatch) {
      const target = WEEKDAYS[weekMatch[2] ?? '']
      if (target) {
        const hasNextPrefix = (weekMatch[1] ?? '').length > 0
        const d = new Date(base)
        // 以周一为一周起点，把 getDay() 的 0..6（周日..周六）映射为 1..7（周一..周日）
        const cur = (d.getDay() + 6) % 7 + 1
        let delta = target - cur
        // 中文口语里「下周一」指**即将到来的那个周一**，而不是"再跳过一整周"。
        // 因此：若目标星期还在本周剩余日子里（delta > 0）就直接取它；
        // 若已过去或正是今天（delta <= 0），则顺延一周。
        // 例：今天周三时，「下周一」= 5 天后的周一；「周五」= 2 天后。
        if (delta <= 0) delta += 7
        d.setDate(d.getDate() + delta)
        setDate(d, weekMatch[0], hasNextPrefix ? `下${weekMatch[2]}` : `周${weekMatch[2]}`)
      }
    }
  }

  // 月/日（含年）
  if (!date) {
    const mdMatch = text.match(/(?:(\d{4})\s*年)?\s*(\d{1,2})\s*月\s*(\d{1,2})\s*[日号]?/)
    if (mdMatch) {
      const y = mdMatch[1] ? Number(mdMatch[1]) : now.getFullYear()
      const m = Number(mdMatch[2])
      const dd = Number(mdMatch[3])
      if (m >= 1 && m <= 12 && dd >= 1 && dd <= 31) {
        const candidate = new Date(y, m - 1, dd)
        // 未写年份且已过去 → 顺延到明年，符合直觉
        if (!mdMatch[1] && candidate.getTime() < base.getTime()) {
          candidate.setFullYear(y + 1)
        }
        setDate(candidate, mdMatch[0], `${m} 月 ${dd} 日`)
      }
    }
  }

  // 纯数字日期 9/30 或 9-30
  if (!date) {
    const slash = text.match(/(?:^|\s)(\d{1,2})[/-](\d{1,2})(?=\s|$)/)
    if (slash) {
      const m = Number(slash[1])
      const dd = Number(slash[2])
      if (m >= 1 && m <= 12 && dd >= 1 && dd <= 31) {
        const candidate = new Date(now.getFullYear(), m - 1, dd)
        if (candidate.getTime() < base.getTime()) candidate.setFullYear(now.getFullYear() + 1)
        setDate(candidate, slash[0].trim(), `${m} 月 ${dd} 日`)
      }
    }
  }

  // ---------------- 5. 组装日期时间 ----------------
  let resolved: Date | null = null
  if (date) {
    resolved = new Date(date)
    if (hour !== null) resolved.setHours(hour, minute, 0, 0)
    else resolved.setHours(0, 0, 0, 0)
  } else if (hour !== null && !hasExplicitDay) {
    // 只写了时刻（如「晚上8点 健身」）→ 默认今天该时刻。
    // 若不这样处理，用户看到的会是"识别到了时间却没有日期"，与直觉不符。
    // 注意：仍会通过 label 明确告知用户落到了今天，用户可在保存前改正。
    resolved = new Date(base)
    resolved.setHours(hour, minute, 0, 0)
    label = '今天'
    matched = null
  }

  // ---------------- 6. 清理标题 ----------------
  let title = text
  for (const m of [colonMatch?.[0], relMatch?.[0]]) {
    if (m) title = title.replace(m, ' ')
  }
  title = title
    .replace(/(上午|早上|中午|下午|傍晚|晚上)?\s*(\d{1,2}|[一二三四五六七八九十]+)\s*[点時时](半)?/g, ' ')
    .replace(/(下+)?(?:周|星期|礼拜)[一二三四五六日天]/g, ' ')
    .replace(/(?:(\d{4})\s*年)?\s*(\d{1,2})\s*月\s*(\d{1,2})\s*[日号]?/g, ' ')
    .replace(/今天|今日|明天|明日|后天/g, ' ')
    .replace(/([0-9一二三四五六七八九十两]+)\s*天后/g, ' ')
    .replace(/\s+/g, ' ')
    .trim()

  return {
    title,
    date: resolved,
    matched,
    label: label && hasTime ? `${label} ${String(hour ?? 0).padStart(2, '0')}:${String(minute).padStart(2, '0')}` : label,
    hasTime,
    tags,
    priority,
  }
}

/** 中文数字转整数（仅支持 1–31，够日期与小时使用） */
export function cnNumberToInt(s: string): number | null {
  if (!s) return null
  if (/^\d+$/.test(s)) return Number(s)
  const digits: Record<string, number> = {
    一: 1, 二: 2, 两: 2, 三: 3, 四: 4, 五: 5, 六: 6, 七: 7, 八: 8, 九: 9, 十: 10,
  }
  if (s === '十') return 10
  if (s.length === 1) return digits[s] ?? null
  // 十一 ~ 三十一
  if (s.startsWith('十')) {
    const rest = digits[s[1] ?? '']
    return rest ? 10 + rest : null
  }
  if (s.includes('十')) {
    const [tens, ones] = s.split('十')
    const t = digits[tens ?? ''] ?? 1
    const o = ones ? (digits[ones] ?? 0) : 0
    return t * 10 + o
  }
  return null
}
