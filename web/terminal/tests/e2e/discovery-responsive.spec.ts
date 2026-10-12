import { expect, test } from '@playwright/test'

const viewports = [
  { name: 'desktop-wide', width: 1920, height: 753, expectsHorizontalTableScroll: false },
  { name: 'desktop', width: 1440, height: 900, expectsHorizontalTableScroll: false },
  { name: 'tablet-landscape', width: 1024, height: 768, expectsHorizontalTableScroll: true },
  { name: 'tablet-portrait', width: 768, height: 1024, expectsHorizontalTableScroll: true },
  { name: 'mobile', width: 390, height: 844, expectsHorizontalTableScroll: false },
  { name: 'mobile-small', width: 360, height: 800, expectsHorizontalTableScroll: false },
]

test('keeps discovery regions contained across supported breakpoints', async ({ page }) => {
  for (const viewport of viewports) {
    await page.setViewportSize({ width: viewport.width, height: viewport.height })
    await page.goto('/')
    await page.locator('.token-avatar img').first().waitFor({ state: 'visible' })
    await page.evaluate(async () => {
      await document.fonts.ready
      await Promise.all([...document.images].map((image) => image.complete ? Promise.resolve() : new Promise((resolve) => {
        image.addEventListener('load', resolve, { once: true })
        image.addEventListener('error', resolve, { once: true })
      })))
    })

    const metrics = await page.evaluate(() => {
      const rect = (selector: string) => {
        const element = document.querySelector<HTMLElement>(selector)
        if (!element) return null
        const box = element.getBoundingClientRect()
        return { top: box.top, right: box.right, bottom: box.bottom, left: box.left }
      }
      const firstAction = document.querySelector<HTMLElement>('.token-row .quick-buy-button')
      const firstActionCell = document.querySelector<HTMLElement>('.token-row td:last-child')
      const action = firstAction?.getBoundingClientRect()
      const actionCell = firstActionCell?.getBoundingClientRect()
      const toolbar = rect('.discovery-toolbar')
      const layout = rect('.discovery-layout')
      const tableScroll = document.querySelector<HTMLElement>('.discovery-table-scroll')
      const chainHints = [...document.querySelectorAll<HTMLElement>('.chain-rail__item .sr-only')]

      return {
        documentFitsViewport: document.documentElement.scrollWidth <= document.documentElement.clientWidth + 1 && document.body.scrollHeight <= document.body.clientHeight + 1,
        toolbarContainsLayout: Boolean(toolbar && layout && layout.top >= toolbar.bottom),
        tableOwnsVerticalScroll: Boolean(tableScroll && tableScroll.scrollHeight > tableScroll.clientHeight),
        tableOwnsHorizontalScroll: Boolean(tableScroll && tableScroll.scrollWidth > tableScroll.clientWidth),
        actionFitsCell: Boolean(action && actionCell && action.left >= actionCell.left - 1 && action.right <= actionCell.right + 1 && action.top >= actionCell.top - 1 && action.bottom <= actionCell.bottom + 1),
        chainHintsHidden: chainHints.length === 3 && chainHints.every((hint) => {
          const style = getComputedStyle(hint)
          return style.position === 'absolute' && style.width === '1px' && style.height === '1px' && style.overflow === 'hidden'
        }),
        avatarsLoaded: [...document.querySelectorAll<HTMLImageElement>('.token-avatar img')].every((image) => image.complete && image.naturalWidth > 0),
      }
    })

    expect(metrics.documentFitsViewport, viewport.name).toBe(true)
    expect(metrics.toolbarContainsLayout, viewport.name).toBe(true)
    expect(metrics.tableOwnsVerticalScroll, viewport.name).toBe(true)
    expect(metrics.tableOwnsHorizontalScroll, viewport.name).toBe(viewport.expectsHorizontalTableScroll)
    expect(metrics.actionFitsCell, viewport.name).toBe(true)
    expect(metrics.chainHintsHidden, viewport.name).toBe(true)
    expect(metrics.avatarsLoaded, viewport.name).toBe(true)
  }
})
