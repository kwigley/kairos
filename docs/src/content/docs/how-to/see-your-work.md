---
title: "See your work"
---

This page describes Kairos 0.13.0.

The page **My work** shows the work that you hold and what needs you now.
To open it, select **My work** in the side menu.

**Before you start:** you are a member of the organization.

## 1. Read "Needs you"

The panel **Needs you** has one row for each item that needs you. The rows
come in this order:

1. **Blocked:** a task that you have a claim on waits on an open blocker.
2. **A notification that you did not read**, for example **Hand-off** or
   **Blocker cleared**. The panel shows the 5 newest. These are the same
   notifications that end each answer of a Kairos tool to your agent. When
   you have more, select **N more unread** to open the **Notifications**
   page.
3. **No change:** a task that you have a claim on has no change for 5 days.

A blocked task shows one time only, as **Blocked**. Select the code of an
item to open the item. The panel changes live: you do not have to load the
page again. When nothing needs you, the panel says so.

## 2. Mark a notification read

Act on the notification first. Then select **Mark read** on its row, or
**Mark all read** above the table.

You, the bell and your agent share the notifications. A notification that
you mark read here is read in the bell too. It does not end the next answer
to your agent. See
[Read notifications as an agent](../read-notifications-as-an-agent/).

## 3. Read your claims and your items

- **My claims** lists the tasks that you have a claim on. You get the claim
  when you move a task into Active. Each row shows the board, the time of the
  claim and the time of the last change. A row has the mark **agent** when
  your agent made the claim.
- **I own** lists the strategies and the initiatives that you own, with the
  column of each.

## 4. Open your profile

Select your name in the top bar. Your profile has these tabs:

| Tab | What it shows |
|---|---|
| **Teams** | Your teams, the delivery board of each, and the latest announcement. |
| **Access** | Your role, and the capabilities that you have on each board. |
| **Notifications** | The kinds of notification that Kairos makes for you. See [Read your notifications](../read-your-notifications/). |
| **Agent keys** | Your agent keys. See [Give an agent your key](../give-an-agent-your-key/). |

## Find the same work with the CLI or MCP

The search has two filters for the work of a person. Each takes `me` or the
UUID of a person.

- `claimed_by` gives the tasks that the person has a claim on.
- `owner` gives the strategies and the initiatives that the person owns.

```sh
kairos search --claimed-by me
kairos search --owner me
```

An agent sends `{"filter": {"claimed_by": "me"}}` to the MCP tool `search`.
See [MCP tools](../../reference/mcp-tools/) and the
[CLI reference](../../reference/cli/).
