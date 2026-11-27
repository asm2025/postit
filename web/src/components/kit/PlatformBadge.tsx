import { usePlatformIcon, type Platform } from '@/features/platforms/queries'

const SIZES = { sm: 'size-5 text-[10px]', md: 'size-7 text-xs', lg: 'size-10 text-base' } as const

interface Props {
  platform: Pick<Platform, 'platform' | 'display_name' | 'brand_color'>
  size?: keyof typeof SIZES
  showName?: boolean
}

/** A platform's icon and name. The first letter on the brand color stands in while the icon loads or is missing. */
export function PlatformBadge({ platform, size = 'md', showName = true }: Props) {
  const icon = usePlatformIcon(platform.platform)
  const box = `inline-flex flex-none items-center justify-center overflow-hidden rounded-md font-bold ${SIZES[size]}`
  return (
    <span className="inline-flex items-center gap-2">
      {icon ? (
        <img src={icon} alt="" className={box} />
      ) : (
        <span aria-hidden="true" className={`${box} text-white`} style={{ backgroundColor: platform.brand_color }}>
          {platform.display_name.slice(0, 1).toUpperCase()}
        </span>
      )}
      {showName ? (
        <span dir="auto">{platform.display_name}</span>
      ) : (
        <span className="sr-only">{platform.display_name}</span>
      )}
    </span>
  )
}
