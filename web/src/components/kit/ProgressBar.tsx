interface Props {
  value: number
  max: number
  /** What is progressing, for screen readers: "Rows checked". */
  label: string
}

/** A bar that fills from the start edge, in either text direction. */
export function ProgressBar({ value, max, label }: Props) {
  const share = max > 0 ? Math.min(100, Math.max(0, (value / max) * 100)) : 0
  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={max}
      aria-valuenow={value}
      className="h-2 w-full overflow-hidden rounded-full bg-raised"
    >
      {/* `inline-size` follows the writing direction, so the fill starts at the start edge. */}
      <div className="h-full bg-pink transition-[inline-size]" style={{ inlineSize: `${String(share)}%` }} />
    </div>
  )
}
