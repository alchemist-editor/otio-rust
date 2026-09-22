'use client'

import * as React from 'react'
import { cva, type VariantProps } from 'class-variance-authority'
import { useRender } from '@base-ui/react/use-render'
import { cn } from '@/lib/cn'

/**
 * The one button.
 *
 * Written in the shadcn/ui convention — a `cva` recipe, variants as props,
 * `render` for changing the element — over Base UI's `useRender`, which is
 * what replaces Radix's `asChild` here. The component lives in this
 * repository rather than in a dependency, which is the point of that
 * convention: a registry component from shadcn or ReUI can be dropped in
 * beside it and will already agree about tokens and class names.
 */
const buttonVariants = cva(
  'inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-[var(--radius)] text-sm font-medium transition-colors disabled:pointer-events-none disabled:opacity-50 [&_svg]:size-4 [&_svg]:shrink-0',
  {
    variants: {
      variant: {
        solid: 'bg-ink text-canvas hover:opacity-90',
        outline: 'border border-edge bg-canvas hover:bg-surface',
        ghost: 'hover:bg-surface',
        accent: 'bg-accent text-canvas hover:opacity-90',
      },
      size: {
        sm: 'h-8 px-3',
        md: 'h-9 px-4',
        icon: 'size-8',
      },
    },
    defaultVariants: { variant: 'outline', size: 'md' },
  },
)

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof buttonVariants> {
  /** An element to render instead of `<button>`, as Base UI spells it. */
  render?: useRender.RenderProp
}

export function Button({ className, variant, size, render, ...props }: ButtonProps) {
  return useRender({
    render: render ?? <button type="button" />,
    props: { className: cn(buttonVariants({ variant, size }), className), ...props },
  })
}

export { buttonVariants }
