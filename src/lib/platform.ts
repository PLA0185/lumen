/** Detect the Android WebView shell; desktop and browser previews use the Windows layout. */
export function isAndroidPlatform(userAgent: string = typeof navigator === 'undefined' ? '' : navigator.userAgent): boolean {
  return /Android/i.test(userAgent)
}