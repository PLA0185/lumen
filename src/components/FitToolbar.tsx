import { useLayoutEffect, useRef, useState, type ReactNode } from 'react'

export function FitToolbar({ children }: { children: ReactNode }) {
  const container = useRef<HTMLDivElement>(null)
  const row = useRef<HTMLDivElement>(null)
  const [size, setSize] = useState({ scale: 1, height: 0 })
  useLayoutEffect(() => {
    const outer = container.current!, inner = row.current!
    const measure = () => {
      // offsetWidth ignores transforms: scaling never feeds back into measurement.
      if (!outer.clientWidth || !inner.offsetWidth) return
      const scale = Math.min(1, outer.clientWidth / inner.offsetWidth)
      const height = inner.offsetHeight * scale
      setSize(previous => previous.scale === scale && previous.height === height ? previous : { scale, height })
    }
    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(outer); observer.observe(inner)
    return () => observer.disconnect()
  }, [])
  return <div className="memos__toolbar" ref={container} style={{ height: size.height ? size.height + 16 : undefined }}>
    <div className="memos__toolbar-row" ref={row} style={{ transform: `scale(${size.scale})` }}>{children}</div>
  </div>
}
