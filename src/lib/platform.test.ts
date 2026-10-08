import { describe, expect, it } from 'vitest'
import { isAndroidPlatform } from './platform'

describe('isAndroidPlatform', () => {
  it('recognizes Android WebView user agents', () => {
    expect(isAndroidPlatform('Mozilla/5.0 (Linux; Android 15; Pixel 9) AppleWebKit/537.36')).toBe(true)
  })

  it('keeps Windows and non-Android webviews on the desktop layout', () => {
    expect(isAndroidPlatform('Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36')).toBe(false)
    expect(isAndroidPlatform('')).toBe(false)
  })
})