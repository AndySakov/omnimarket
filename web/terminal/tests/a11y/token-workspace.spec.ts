import AxeBuilder from '@axe-core/playwright'
import { expect, test } from '@playwright/test'

test('token workspace has no serious or critical accessibility violations', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 })
  await page.goto('/')
  await page.locator('.token-name').filter({ hasText: 'NovaSet' }).first().click()

  const results = await new AxeBuilder({ page }).analyze()
  const blockingViolations = results.violations.filter(
    (violation) => violation.impact === 'serious' || violation.impact === 'critical',
  )

  expect(blockingViolations).toEqual([])
})
