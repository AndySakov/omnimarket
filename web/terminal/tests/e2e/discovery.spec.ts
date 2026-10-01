import { expect, test } from '@playwright/test'

test('keeps the terminal fixed while the token stream scrolls', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 })
  await page.goto('/')

  const metrics = await page.evaluate(() => {
    const body = document.body
    const scrollRegion = document.querySelector<HTMLElement>('.discovery-table-scroll')

    return {
      documentFitsViewport: body.scrollHeight <= body.clientHeight + 1,
      tokenRegionScrolls: Boolean(scrollRegion && scrollRegion.scrollHeight > scrollRegion.clientHeight),
    }
  })

  expect(metrics.documentFitsViewport).toBe(true)
  expect(metrics.tokenRegionScrolls).toBe(true)
  await expect(page.locator('.token-avatar img')).toHaveCount(6)
  await expect(page.locator('.chain-rail__mark img')).toHaveCount(5)
})

test('opens the selected Discover token in its token workspace', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 })
  await page.goto('/')

  const novaSet = page.getByRole('button', { name: 'NovaSet', exact: true })
  await expect(novaSet).toBeVisible()
  await novaSet.click()

  await expect(page.getByRole('main', { name: 'NovaSet token workspace' })).toBeVisible()
  await expect(page.getByRole('heading', { name: 'NovaSet' })).toBeVisible()
  await expect(page.getByRole('heading', { name: 'NOVA', exact: true })).toBeVisible()
  await expect(page.getByRole('tab', { name: 'Trades' })).toBeVisible()
})
