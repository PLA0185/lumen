/** 圆形按钮自身提供绿色底；这里仅绘制清晰的白色对勾。 */
export function CheckMark({ size = 11 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 12 12" aria-hidden="true">
      <path
        d="M2.5 6.2l2.3 2.3L9.5 3.8"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  )
}
