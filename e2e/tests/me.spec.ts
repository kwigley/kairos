// The personal pages:
//
//   1. "My work" in the nav opens /me;
//   2. a hand-off to alice shows live in "Needs you" with its kind, the
//      same notification that her agent sees, and Mark read clears it for
//      both;
//   3. a task that alice moves into Active shows in "My claims";
//   4. the name in the top bar opens the profile, and its tabs show the
//      teams, the access, the notification settings and the agent keys;
//   5. /agent-keys goes to the tab Agent keys of the profile.
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
  await page.waitForURL((url) => url.pathname.startsWith('/boards'), {
    timeout: 30_000,
  });
}

async function post(token: string, path: string, body: unknown): Promise<any> {
  const res = await fetch(`${GUI}${path}`, {
    method: 'POST',
    headers: { authorization: `Bearer ${token}`, 'content-type': 'application/json' },
    body: JSON.stringify(body),
  });
  const text = await res.text();
  expect(res.ok, text).toBeTruthy();
  return text ? JSON.parse(text) : null;
}

async function moveTo(token: string, code: string, column: string): Promise<void> {
  await post(token, `/api/tasks/${code}/transition`, { to_column_id: column });
}

async function unread(token: string): Promise<number> {
  const res = await fetch(`${GUI}/api/me/notifications`, {
    headers: { authorization: `Bearer ${token}` },
  });
  expect(res.ok).toBeTruthy();
  return (await res.json()).unread;
}

test('My work shows the claims, and the profile holds the settings', async ({ page }) => {
  const token = await mintToken();
  const bob = await mintToken({ email: 'bob@kairos.test', password: 'bob-password' });
  const board = await loadBoard(GUI, token, (b) => b.slug === 'platform-delivery');
  const todo = board.columnId.get('Todo');
  const active = board.columnId.get('Active');
  if (!todo || !active) throw new Error('platform-delivery has no Todo or Active column');
  const title = `E2E my work ${Date.now()}`;
  const created = await createTask(GUI, token, { title, boardId: board.id });
  const code: string = created.short_code;
  const given = await createTask(GUI, bob, { title: `E2E handed ${Date.now()}`, boardId: board.id });
  const handed: string = given.short_code;

  try {
    if (created.column_id !== todo) await moveTo(token, code, todo);
    await moveTo(token, code, active);

    await test.step('My work in the nav opens /me', async () => {
      await login(page);
      await page.locator('a[href="/me"]').first().click();
      await page.waitForURL((url) => url.pathname === '/me');
      await expect(page.getByRole('heading', { name: 'My work' })).toBeVisible();
    });

    await test.step('a hand-off shows live in Needs you, and Mark read clears it', async () => {
      await post(token, '/api/me/notifications/read', { all: true });
      if (given.column_id !== todo) await moveTo(bob, handed, todo);
      await moveTo(bob, handed, active);
      await post(bob, `/api/tasks/${handed}/hand-off`, { to: 'alice@kairos.test' });
      const row = page.locator('[data-testid="needs-you"] tr', { hasText: handed });
      // No reload: the page reads the work again on the event.
      await expect(row).toContainText('Hand-off', { timeout: 20_000 });
      expect(await unread(token)).toBe(1);
      await row.getByRole('button', { name: 'Mark read', exact: true }).click();
      await expect(row.filter({ hasText: 'Hand-off' })).toHaveCount(0, { timeout: 20_000 });
      // The mark is shared: the bell, and the block at the end of each MCP
      // answer of her agent, read the same state.
      expect(await unread(token)).toBe(0);
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
    for (const [owner, item] of [
      [token, code],
      [bob, handed],
    ]) {
      await fetch(`${GUI}/api/tasks/${item}`, {
        method: 'DELETE',
        headers: { authorization: `Bearer ${owner}` },
      });
    }
  }
});
