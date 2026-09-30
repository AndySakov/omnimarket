import { expect, test } from '@playwright/test'

test('navigates between terminal workspaces', async ({ page }) => {
  await page.goto('/')

  await expect(page.getByRole('main', { name: 'Discover market radar' })).toBeVisible()
  await page.getByRole('button', { name: 'Portfolio' }).click()

  await expect(page.getByRole('button', { name: 'Portfolio' })).toHaveAttribute('aria-current', 'page')
  await expect(page.getByRole('main')).toContainText('Portfolio workspace')
})
