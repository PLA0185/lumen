import { protectedText } from '../lib/no-orphan'
export function SafeText({ children }: { children: string }) {
  return <>{protectedText(children).map((part, i) => part.protect ? <span className="no-orphan" key={i}>{part.text}</span> : part.text)}</>
}
