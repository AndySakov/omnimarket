import { expect, test } from '@playwright/test'

test('the engine status bar moves with every fixture block', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 })
  await page.goto('/')
  const bar = page.getByRole('button', { name: /^Engine:/ })
  await expect(bar).toHaveAttribute('data-state', 'live')
  const first = await bar.getAttribute('aria-label')
  // The fixture stream makes a block every two seconds.
  await expect.poll(async () => bar.getAttribute('aria-label'), { timeout: 5_000 }).not.toBe(first)
  await expect(bar).toHaveAccessibleName(/^Engine: Fixtures\. Base, block 36,120,45\d/)
})

for (const route of ['Discover', 'Portfolio', 'Wallets']) {
  test(`"How it works" opens in one click from ${route}`, async ({ page }) => {
    await page.goto('/')
    if (route !== 'Discover') await page.getByRole('button', { name: route, exact: true }).first().click()
    await page.getByRole('button', { name: 'How it works' }).click()
    await expect(page.getByRole('dialog', { name: 'How it works' })).toBeVisible()
  })
}

test('"How it works" is reachable on a phone', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 })
  await page.goto('/')
  await page.getByRole('button', { name: 'How it works' }).click()
  const dialog = page.getByRole('dialog', { name: 'How it works' })
  await expect(dialog).toBeVisible()
  await expect(dialog.getByRole('link', { name: 'The decision log' })).toBeAttached()
})
