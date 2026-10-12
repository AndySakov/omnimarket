import { expect, test } from '@playwright/test'

test('keeps the terminal fixed while the token stream scrolls', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 })
  await page.goto('/')
  // Fixture mode starts its mock worker before the app renders.
  await expect(page.locator('.discovery-table-scroll')).toBeVisible()

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
  await expect(page.locator('.chain-rail__mark img')).toHaveCount(3)
})

test('opens the selected Discover token in its token workspace', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 })
  await page.goto('/')

  const novaSet = page.locator('.token-name').filter({ hasText: 'NovaSet' }).first()
  await expect(novaSet).toBeVisible()
  await novaSet.click()

  await expect(page.getByRole('main', { name: 'NovaSet token workspace' })).toBeVisible()
  await expect(page.getByRole('heading', { name: 'NovaSet' })).toBeVisible()
  await expect(page.getByRole('heading', { name: 'NOVA', exact: true })).toBeVisible()
  await expect(page.getByRole('tab', { name: 'Trades' })).toBeVisible()
})

test('opens the token workspace when the token row body is clicked', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 })
  await page.goto('/')

  const novaSetRow = page.getByRole('row').filter({ hasText: 'NovaSet' })
  await expect(novaSetRow).toBeVisible()
  await novaSetRow.getByText('$12.4M', { exact: true }).click()

  await expect(page.getByRole('main', { name: 'NovaSet token workspace' })).toBeVisible()
})
