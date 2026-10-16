import type { ViewId } from '../lib/types'
import { REPEAT_VIEWS } from '../lib/repeat-views'

interface RepeatViewTabsProps {
  current: ViewId
  onSelect: (view: ViewId) => void
}

export function RepeatViewTabs({ current, onSelect }: RepeatViewTabsProps) {
  return (
    <nav className="repeat-view-tabs" aria-label="重复任务频率">
      {REPEAT_VIEWS.map((item) => (
        <button
          key={item.id}
          type="button"
          className="repeat-view-tabs__item"
          aria-label={item.title}
          aria-current={current === item.id ? 'page' : undefined}
          onClick={() => onSelect(item.id)}
        >
          {item.label}
        </button>
      ))}
    </nav>
  )
}
