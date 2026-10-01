import AxeBuilder from '@axe-core/playwright'
import { expect, test } from '@playwright/test'

test('the account panel has no serious or critical accessibility violations', async ({ page }) => {
  await page.goto('/')
  await page.getByRole('button', { name: 'Continue as guest' }).click()
  await page.getByRole('button', { name: /^Open account menu: guest/ }).click()
  await expect(page.getByRole('dialog', { name: 'Account' })).toBeVisible()

  const results = await new AxeBuilder({ page }).include('.account-panel').analyze()
  const blocking = results.violations.filter((v) => v.impact === 'serious' || v.impact === 'critical')
  expect(blocking).toEqual([])
})
