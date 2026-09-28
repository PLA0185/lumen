type TextField = HTMLInputElement | HTMLTextAreaElement

function textField(target: EventTarget | null): TextField | null {
  if (target instanceof HTMLTextAreaElement)
    return target.disabled || target.readOnly ? null : target
  if (
    target instanceof HTMLInputElement &&
    ['text', 'search', 'url', 'tel', 'password'].includes(target.type)
  ) {
    return target.disabled || target.readOnly ? null : target
  }
  return null
}

/** Read at the paste gesture, with no clipboard cache or focus-time autofill. */
export function installFreshPaste(
  readText: () => Promise<string>,
  onError: () => void,
  doc = document,
): () => void {
  let generation = 0
  let disposed = false
  const paste = async (field: TextField) => {
    const request = ++generation
    const value = field.value
    const start = field.selectionStart ?? value.length
    const end = field.selectionEnd ?? start
    try {
      const text = await readText()
      if (
        disposed ||
        request !== generation ||
        !field.isConnected ||
        doc.activeElement !== field ||
        field.value !== value ||
        field.selectionStart !== start ||
        field.selectionEnd !== end
      )
        return
      if (!text) return
      let insert =
        field instanceof HTMLTextAreaElement
          ? text
          : text.replace(/\r\n|[\r\n]/g, ' ')
      if (field.maxLength >= 0)
        insert = insert.slice(
          0,
          Math.max(0, field.maxLength - (value.length - (end - start))),
        )
      // Chromium's editing command keeps native undo history and emits input.
      if (
        typeof doc.execCommand === 'function' &&
        doc.execCommand('insertText', false, insert)
      )
        return
      const prototype =
        field instanceof HTMLTextAreaElement
          ? HTMLTextAreaElement.prototype
          : HTMLInputElement.prototype
      Object.getOwnPropertyDescriptor(prototype, 'value')?.set?.call(
        field,
        value.slice(0, start) + insert + value.slice(end),
      )
      field.setSelectionRange(start + insert.length, start + insert.length)
      field.dispatchEvent(
        new InputEvent('input', {
          bubbles: true,
          inputType: 'insertFromPaste',
          data: insert,
        }),
      )
    } catch {
      if (!disposed && request === generation) onError()
    }
  }
  const keydown = (event: KeyboardEvent) => {
    if (
      event.defaultPrevented ||
      event.isComposing ||
      event.altKey ||
      !(event.ctrlKey || event.metaKey) ||
      event.key.toLowerCase() !== 'v'
    )
      return
    const field = textField(event.target)
    if (!field) return
    event.preventDefault()
    if (!event.repeat) void paste(field)
  }
  const onPaste = (event: ClipboardEvent) => {
    if (event.defaultPrevented) return
    const field = textField(event.target)
    if (!field) return
    event.preventDefault()
    void paste(field)
  }
  doc.addEventListener('keydown', keydown, true)
  doc.addEventListener('paste', onPaste, true)
  return () => {
    disposed = true
    ++generation
    doc.removeEventListener('keydown', keydown, true)
    doc.removeEventListener('paste', onPaste, true)
  }
}
