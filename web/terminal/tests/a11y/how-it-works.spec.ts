import AxeBuilder from '@axe-core/playwright'
import { expect, test } from '@playwright/test'

test('the "How it works" view has no serious or critical accessibility violations', async ({ page }) => {
  await page.goto('/')
  await page.getByRole('button', { name: 'How it works' }).click()
  await expect(page.getByRole('dialog', { name: 'How it works' })).toBeVisible()

  const results = await new AxeBuilder({ page }).include('[role="dialog"]').analyze()
  const blockingViolations = results.violations.filter(
    (violation) => violation.impact === 'serious' || violation.impact === 'critical',
  )

  expect(blockingViolations).toEqual([])
})
