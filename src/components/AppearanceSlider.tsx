import { useRef, type ComponentProps } from 'react'

type Props = Omit<ComponentProps<'input'>, 'onChange'> & { onPreview: (value: string) => void; onCommit: (value: string) => void }
export function AppearanceSlider({ onPreview, onCommit, ...props }: Props) {
  const dragging = useRef(false)
  const finish = (value: string) => {
    if (!dragging.current) return
    dragging.current = false
    onCommit(value)
  }
  return <input {...props} type="range"
    onPointerDown={e => { dragging.current = true; e.currentTarget.setPointerCapture?.(e.pointerId) }}
    onPointerUp={e => finish(e.currentTarget.value)}
    onPointerCancel={e => finish(e.currentTarget.value)}
    onLostPointerCapture={e => finish(e.currentTarget.value)}
    onBlur={e => finish(e.currentTarget.value)}
    onChange={e => { onPreview(e.target.value); if (!dragging.current) onCommit(e.target.value) }} />
}
