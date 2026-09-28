/** 圆形按钮自身提供绿色底；这里仅绘制清晰的白色对勾。 */
export function CheckMark({ size = 13 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 12 12" aria-hidden="true">
      <path
        d="M1.8 6.2l2.9 2.7L10.2 3.3"
        fill="none"
        stroke="currentColor"
        strokeWidth="2.6"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  )
}
