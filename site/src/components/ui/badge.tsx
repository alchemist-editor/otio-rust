import * as React from 'react'
import { cva, type VariantProps } from 'class-variance-authority'
import { cn } from '@/lib/cn'

const badgeVariants = cva(
  'inline-flex items-center rounded-full border px-2 py-0.5 text-[0.7rem] font-medium tracking-wide',
  {
    variants: {
      variant: {
        neutral: 'border-edge bg-surface text-muted',
        accent: 'border-transparent bg-accent-soft text-accent',
        outline: 'border-edge text-muted',
      },
    },
    defaultVariants: { variant: 'neutral' },
  },
)

export function Badge({
  className,
  variant,
  ...props
}: React.HTMLAttributes<HTMLSpanElement> & VariantProps<typeof badgeVariants>) {
  return <span className={cn(badgeVariants({ variant }), className)} {...props} />
}
