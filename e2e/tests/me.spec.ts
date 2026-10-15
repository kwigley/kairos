// The personal pages:
//
//   1. the sign-in lands on /me, "My work";
//   2. a task that alice moves into Active shows in "My claims";
//   3. the name in the top bar opens the profile, and its tabs show the
//      teams, the access, the notification settings and the agent keys;
//   4. /agent-keys goes to the tab Agent keys of the profile.
//
// The task is new for each run (a timestamped title), so a retry starts
// clean.

import { test, expect, Page } from '@playwright/test';
import { mintToken } from '../helpers/auth';
import { createTask, loadBoard } from '../helpers/api';

const GUI = process.env.E2E_GUI_BASE_URL ?? 'http://localhost:41080';

async function login(page: Page): Promise<void> {
  await page.goto('/');
  await page.waitForSelector('#login', { timeout: 30_000 });
  await page.fill('#login', 'alice@kairos.test');
  await page.fill('#password', 'alice-password');
  await page.click('#submit-login');
  await page.waitForURL((url) => url.pathname === '/me', { timeout: 30_000 });
}

async function moveTo(token: string, code: string, column: string): Promise<void> {
  const res = await fetch(`${GUI}/api/tasks/${code}/transition`, {
    method: 'POST',
    headers: { authorization: `Bearer ${token}`, 'content-type': 'application/json' },
    body: JSON.stringify({ to_column_id: column }),
  });
  expect(res.ok, await res.text()).toBeTruthy();
}

test('My work shows the claims, and the profile holds the settings', async ({ page }) => {
  const token = await mintToken();
  const board = await loadBoard(GUI, token, (b) => b.slug === 'platform-delivery');
  const todo = board.columnId.get('Todo');
  const active = board.columnId.get('Active');
  if (!todo || !active) throw new Error('platform-delivery has no Todo or Active column');
  const title = `E2E my work ${Date.now()}`;
  const created = await createTask(GUI, token, { title, boardId: board.id });
  const code: string = created.short_code;

  try {
    if (created.column_id !== todo) await moveTo(token, code, todo);
    await moveTo(token, code, active);

    await test.step('the sign-in lands on My work', async () => {
      await login(page);
      await expect(page.getByRole('heading', { name: 'My work' })).toBeVisible();
    });

    await test.step('the claimed task is in My claims', async () => {
      const claims = page.locator('[data-testid="my-claims"]');
      await expect(claims).toContainText(code, { timeout: 20_000 });
      await expect(claims).toContainText(title);
      await claims.getByRole('link', { name: code, exact: true }).click();
      await page.waitForURL(new RegExp(`/items/${code}$`));
    });

    await test.step('the name opens the profile', async () => {
      await page.locator('a[href="/me/profile"]').first().click();
      await page.waitForURL(/\/me\/profile$/);
      await expect(page.locator('[data-testid="profile-teams"]')).toBeVisible();
    });

    await test.step('the tabs of the profile', async () => {
      await page.getByRole('tab', { name: 'Access', exact: true }).click();
      await expect(page.getByText('Your boards')).toBeVisible();
      await page.getByRole('tab', { name: 'Notifications', exact: true }).click();
      await expect(page.locator('[data-testid="notification-settings"]')).toBeVisible();
      await page.getByRole('tab', { name: 'Agent keys', exact: true }).click();
      await expect(page.getByRole('button', { name: 'Make key', exact: true })).toBeVisible();
    });

    await test.step('/agent-keys goes to the tab', async () => {
      await page.goto('/agent-keys');
      await page.waitForURL(/\/me\/profile\?tab=agent-keys/);
      await expect(page.getByRole('button', { name: 'Make key', exact: true })).toBeVisible();
    });
  } finally {
    await fetch(`${GUI}/api/tasks/${code}`, {
      method: 'DELETE',
      headers: { authorization: `Bearer ${token}` },
    });
  }
});
