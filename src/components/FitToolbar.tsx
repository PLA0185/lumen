import type { ReactNode } from 'react'

export function FitToolbar({ children }: { children: ReactNode }) {
  return <div className="memos__toolbar">
    <div className="memos__toolbar-row">{children}</div>
  </div>
}
