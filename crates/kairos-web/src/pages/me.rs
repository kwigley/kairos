//! The personal pages of the signed-in person.
//!
//! - `/me`, "My work", in the nav: what needs the person now, the
//!   tasks that the person has a claim on, and the strategies and the
//!   initiatives that the person owns.
//! - `/me/profile` is the person and the settings, in tabs (`?tab=`): the
//!   teams, the access on the boards, the notification settings and the
//!   agent keys. The name in the top bar links to it.
//!
//! The work comes from two searches (`claimed_by: "me"` and `owner:
//! "me"`). The search gives the claim of each task and the open blockers of
//! each item (`blocks_summary`), so the page needs no read for each item.

use aurora_dark::components::{
    Anchor, Button, Empty, ErrorState, Group, Loading, Panel, Pill, RelativeTime, SimpleGrid,
    Stack, Table, TableRow, Text,
};
use aurora_dark::frame::{Card, PageHeader, TabItem, Tabs};
use aurora_dark::tokens::{ApiError, token};
use leptos::prelude::*;
use leptos_router::hooks::use_query_map;

use crate::api;
use crate::auth::use_auth;

pub(crate) mod data;

use data::{MyWork, Need, Reason, STALE_DAYS, UNREAD_SHOWN, WorkItem};

/// `/me`: the work of the person.
#[component]
pub fn MyWorkPage() -> impl IntoView {
    let auth = use_auth();
    let whoami = use_context::<LocalResource<Result<api::Whoami, ApiError>>>();
    let reload = RwSignal::new(0u32);
    let work = LocalResource::new(move || {
        let _ = auth.token();
        reload.get();
        data::load_work(auth)
    });
    let unread_reload = RwSignal::new(0u32);
    let unread = LocalResource::new(move || {
        let _ = auth.token();
        reload.get();
        unread_reload.get();
        data::load_unread(auth)
    });
    let retry = Callback::new(move |_| reload.update(|n| *n += 1));

    // As the bell: each event of the person (a new notification, a mark
    // read on another surface) reads the work again. A hand-off makes a
    // notification and changes the claims, so both lists read again.
    let guard = crate::pages::boards::live::subscribe_person_events(auth, move || {
        reload.try_update(|n| *n += 1);
    });
    let live_guard: StoredValue<Option<crate::pages::boards::live::LiveBoardGuard>, LocalStorage> =
        StoredValue::new_local(Some(guard));
    on_cleanup(move || live_guard.set_value(None));

    // Mark read, as in the bell: `None` marks each unread notification.
    let busy = RwSignal::new(false);
    let mark_error: RwSignal<Option<ApiError>> = RwSignal::new(None);
    let on_mark = Callback::new(move |id: Option<String>| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        leptos::task::spawn_local(async move {
            let result = crate::pages::notifications::mark(auth, id.as_deref()).await;
            busy.set(false);
            mark_error.set(result.err());
            unread_reload.update(|n| *n += 1);
        });
    });
    let teams = move || {
        whoami
            .and_then(|w| w.get())
            .and_then(|r| r.ok())
            .map(|me| me.teams)
            .unwrap_or_default()
    };

    view! {
        <PageHeader title="My work" sub="What needs you now, and the work that you hold."/>
        <Stack gap="md">
        {move || match (work.get(), unread.get()) {
            (None, _) | (_, None) => view! { <Loading/> }.into_any(),
            (Some(Err(error)), _) | (_, Some(Err(error))) => {
                view! { <ErrorState error on_retry=retry/> }.into_any()
            }
            (Some(Ok(work)), Some(Ok(unread))) => {
                let needs = data::needs_you(&work.claims, &unread.newest, &data::stale_cutoff());
                let more = unread.more();
                let claims = work.clone();
                view! {
                    <Stack gap="md">
                        <Panel title="Needs you"
                            caption=format!("Blocked claims first, then the {UNREAD_SHOWN} newest \
                                             unread notifications (your agent sees the same), \
                                             then claims with no change for {STALE_DAYS} days.")>
                            {move || mark_error.get().map(|error| view! { <ErrorState error/> })}
                            <NeedsTable needs more busy on_mark/>
                        </Panel>
                        <Panel title="My claims"
                            caption="The tasks that you hold. You get the claim when you move a \
                                     task into Active.">
                            <ClaimsTable work=claims/>
                        </Panel>
                        <Panel title="I own"
                            caption="The strategies and the initiatives that you answer for.">
                            <OwnedTable work/>
                        </Panel>
                    </Stack>
                }
                .into_any()
            }
        }}
        {move || {
            let teams = teams();
            (!teams.is_empty()).then(|| view! {
                <Group gap="sm" wrap=true>
                    <Text size="sm" dimmed=true>"Your teams:"</Text>
                    {teams.into_iter().map(|team| view! {
                        <Anchor href=format!("/teams/{}", team.slug)>{team.name}</Anchor>
                    }).collect_view()}
                </Group>
            })
        }}
        </Stack>
    }
}

/// The words and the color of a row: the kind of a notification, as the
/// bell and the MCP block name it.
fn reason_pill(need: &Need) -> (&'static str, &'static str) {
    match (need.reason, &need.notification) {
        (Reason::Blocked, _) => ("Blocked", token::BAD),
        (Reason::Unread, Some(note)) => (
            crate::pages::notifications::kind_label(&note.kind),
            token::ICE,
        ),
        (Reason::Unread, None) => ("Unread", token::ICE),
        (Reason::Stale, _) => ("No change", token::GOLD),
    }
}

/// "Needs you". `more` is the count of the unread notifications that the
/// table does not show. `on_mark` marks one notification read, or each one
/// with `None`.
#[component]
fn NeedsTable(
    needs: Vec<Need>,
    more: i64,
    busy: RwSignal<bool>,
    on_mark: Callback<Option<String>>,
) -> impl IntoView {
    if needs.is_empty() {
        return view! { <Empty message="Nothing needs you now."/> }.into_any();
    }
    let any_unread = needs.iter().any(|n| n.notification.is_some());
    view! {
        <div data-testid="needs-you">
            {any_unread.then(|| view! {
                <Group justify="end" gap="sm">
                    {(more > 0).then(|| view! {
                        <Anchor href="/notifications">{format!("{more} more unread")}</Anchor>
                    })}
                    <Button variant="default" size="xs" disabled=Signal::derive(move || busy.get())
                        on_click=Callback::new(move |_| on_mark.run(None))>
                        "Mark all read"
                    </Button>
                </Group>
            })}
            <Table label="Needs you">
                <thead><tr>
                    <th>"Why"</th><th>"Item"</th><th>"What"</th><th>"When"</th><th>""</th>
                </tr></thead>
                <tbody>
                    {needs.into_iter().map(|need| {
                        let (label, color) = reason_pill(&need);
                        let id = need.notification.map(|note| note.id);
                        view! {
                            <TableRow>
                                <td><Pill color=color>{label}</Pill></td>
                                <td>{need.short_code.map(|code| view! {
                                    <Anchor href=format!("/items/{code}")>{code.clone()}</Anchor>
                                })}</td>
                                <td><Text size="sm">{need.text}</Text></td>
                                <td><RelativeTime iso=need.at/></td>
                                <td>{id.map(|id| view! {
                                    <Button variant="default" size="xs"
                                        disabled=Signal::derive(move || busy.get())
                                        on_click=Callback::new(move |_| on_mark.run(Some(id.clone())))>
                                        "Mark read"
                                    </Button>
                                })}</td>
                            </TableRow>
                        }
                    }).collect_view()}
                </tbody>
            </Table>
        </div>
    }
    .into_any()
}

#[component]
fn ClaimsTable(work: MyWork) -> impl IntoView {
    let tasks = work.claims.results.tasks.clone();
    let more = work.claims.more_than_shown();
    if tasks.is_empty() {
        return view! {
            <Empty message="You hold no task. Move a task into Active on a board to claim it."
                href="/boards" link="Boards"/>
        }
        .into_any();
    }
    view! {
        <div data-testid="my-claims">
            {more.map(|text| view! { <Text size="sm" dimmed=true>{text}</Text> })}
            <Table label="My claims">
                <thead><tr>
                    <th>"Task"</th><th>"Board"</th><th>"Claimed"</th><th>"Last change"</th><th>""</th>
                </tr></thead>
                <tbody>
                    {tasks.into_iter().map(|task| {
                        let board = work.board(&task.board_id);
                        let blockers = work.claims.blocked_by(&task.short_code);
                        let claim = task.claim.clone();
                        view! {
                            <TableRow>
                                <td><ItemCell item=task.clone()/></td>
                                <td><Anchor href=format!("/boards/{}", board.slug)>{board.name}</Anchor></td>
                                <td><RelativeTime iso=claim.as_ref().map(|c| c.claimed_at.clone())/></td>
                                <td><RelativeTime iso=task.updated_at/></td>
                                <td>
                                    <Group gap="xs">
                                        {(blockers > 0).then(|| view! {
                                            <Pill color=token::BAD>{format!("blocked by {blockers}")}</Pill>
                                        })}
                                        {claim.is_some_and(|c| c.agent).then(|| view! {
                                            <Pill color=token::MUTED>"agent"</Pill>
                                        })}
                                    </Group>
                                </td>
                            </TableRow>
                        }
                    }).collect_view()}
                </tbody>
            </Table>
        </div>
    }
    .into_any()
}

#[component]
fn OwnedTable(work: MyWork) -> impl IntoView {
    let items: Vec<(&'static str, WorkItem)> = work
        .owned
        .results
        .strategies
        .iter()
        .map(|i| ("Strategy", i.clone()))
        .chain(
            work.owned
                .results
                .initiatives
                .iter()
                .map(|i| ("Initiative", i.clone())),
        )
        .collect();
    if items.is_empty() {
        return view! { <Empty message="You own no strategy or initiative."/> }.into_any();
    }
    let more = work.owned.more_than_shown();
    view! {
        <div data-testid="i-own">
            {more.map(|text| view! { <Text size="sm" dimmed=true>{text}</Text> })}
            <Table label="I own">
                <thead><tr>
                    <th>"Item"</th><th>"Type"</th><th>"Board"</th><th>"Column"</th><th>"Last change"</th>
                </tr></thead>
                <tbody>
                    {items.into_iter().map(|(kind, item)| {
                        let board = work.board(&item.board_id);
                        let column = work.column(&item.column_id);
                        view! {
                            <TableRow>
                                <td><ItemCell item=item.clone()/></td>
                                <td><Pill color=token::VIOLET>{kind}</Pill></td>
                                <td><Anchor href=format!("/boards/{}", board.slug)>{board.name}</Anchor></td>
                                <td><Text size="sm">{column}</Text></td>
                                <td><RelativeTime iso=item.updated_at/></td>
                            </TableRow>
                        }
                    }).collect_view()}
                </tbody>
            </Table>
        </div>
    }
    .into_any()
}

#[component]
fn ItemCell(item: WorkItem) -> impl IntoView {
    view! {
        <Group gap="xs">
            <Anchor href=format!("/items/{}", item.short_code)>{item.short_code.clone()}</Anchor>
            <Text size="sm">{item.title}</Text>
        </Group>
    }
}

/// A tab of the profile. `?tab=` holds its key, so a link opens one tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileTab {
    Teams,
    Access,
    Notifications,
    AgentKeys,
}

impl ProfileTab {
    /// The tabs, in the order of the tab list.
    const ALL: [ProfileTab; 4] = [
        ProfileTab::Teams,
        ProfileTab::Access,
        ProfileTab::Notifications,
        ProfileTab::AgentKeys,
    ];

    /// The value of `?tab=`.
    fn key(self) -> &'static str {
        match self {
            ProfileTab::Teams => "teams",
            ProfileTab::Access => "access",
            ProfileTab::Notifications => "notifications",
            ProfileTab::AgentKeys => "agent-keys",
        }
    }

    fn label(self) -> &'static str {
        match self {
            ProfileTab::Teams => "Teams",
            ProfileTab::Access => "Access",
            ProfileTab::Notifications => "Notifications",
            ProfileTab::AgentKeys => "Agent keys",
        }
    }

    /// The path that opens the tab.
    pub fn href(self) -> String {
        format!("/me/profile?tab={}", self.key())
    }

    /// The tab of a `?tab=` value: the first tab for no value or an unknown
    /// one. Pure, host-tested.
    fn from_query(value: Option<&str>) -> Self {
        value
            .and_then(|v| Self::ALL.into_iter().find(|tab| tab.key() == v))
            .unwrap_or(ProfileTab::Teams)
    }
}

/// `/me/profile`: the person and the settings.
#[component]
pub fn ProfilePage() -> impl IntoView {
    let whoami = use_context::<LocalResource<Result<api::Whoami, ApiError>>>();
    let query = use_query_map();
    let tab = Memo::new(move |_| ProfileTab::from_query(query.read().get("tab").as_deref()));
    let tab_value = RwSignal::new(tab.get_untracked().key().to_string());
    Effect::new(move |_| tab_value.set(tab.get().key().to_string()));
    let tabs: Vec<TabItem> = ProfileTab::ALL
        .into_iter()
        .map(|tab| TabItem::new(tab.key(), tab.label()).href(tab.href()))
        .collect();

    view! {
        {move || match whoami.and_then(|w| w.get()) {
            None => view! { <Loading/> }.into_any(),
            Some(Err(error)) => view! { <ErrorState error/> }.into_any(),
            Some(Ok(me)) => {
                let role = me.organization.role.clone();
                let org = me.organization.slug.clone();
                view! {
                    <PageHeader title=me.user.display_name.clone() sub=me.user.email.clone()
                        meta=Box::new(move || view! {
                            <Group gap="xs">
                                <Pill color=token::ICE>{org.clone()}</Pill>
                                <Pill color=token::VIOLET>{role.clone()}</Pill>
                            </Group>
                        }.into_any())/>
                }.into_any()
            }
        }}
        <Tabs tabs value=tab_value label="Profile"/>
        <div class="kairos-profile-tab">
            {move || match tab.get() {
                ProfileTab::Teams => view! { <TeamsTab/> }.into_any(),
                ProfileTab::Access => view! { <AccessTab/> }.into_any(),
                ProfileTab::Notifications => view! { <crate::pages::notifications::NotificationSettingsPanel/> }.into_any(),
                ProfileTab::AgentKeys => view! { <crate::pages::agent_keys::AgentKeysPanel/> }.into_any(),
            }}
        </div>
    }
}

#[component]
fn TeamsTab() -> impl IntoView {
    let auth = use_auth();
    let whoami = use_context::<LocalResource<Result<api::Whoami, ApiError>>>();
    let teams = LocalResource::new(move || {
        let _ = auth.token();
        let mine = whoami
            .and_then(|w| w.get())
            .and_then(|r| r.ok())
            .map(|me| me.teams)
            .unwrap_or_default();
        data::load_teams(auth, mine)
    });
    view! {
        {move || match teams.get() {
            None => view! { <Loading/> }.into_any(),
            Some(Err(error)) => view! { <ErrorState error/> }.into_any(),
            Some(Ok(teams)) if teams.is_empty() => view! {
                <Empty message="You are in no team. An organization admin adds people to teams."/>
            }.into_any(),
            Some(Ok(teams)) => view! {
                <div data-testid="profile-teams">
                    <SimpleGrid cols=2>
                    {teams.into_iter().map(|team| view! {
                        <Card href=format!("/teams/{}", team.slug) title=team.name label="Team">
                            <Stack gap="xs">
                                <Text size="sm" dimmed=true>
                                    {team.announcement.unwrap_or_else(|| "No announcement.".to_string())}
                                </Text>
                                {team.board.map(|board| view! {
                                    <Anchor href=format!("/boards/{board}")>"Delivery board"</Anchor>
                                })}
                            </Stack>
                        </Card>
                    }).collect_view()}
                    </SimpleGrid>
                </div>
            }.into_any(),
        }}
    }
}

#[component]
fn AccessTab() -> impl IntoView {
    let whoami = use_context::<LocalResource<Result<api::Whoami, ApiError>>>();
    view! {
        {move || match whoami.and_then(|w| w.get()) {
            None => view! { <Loading/> }.into_any(),
            Some(Err(error)) => view! { <ErrorState error/> }.into_any(),
            Some(Ok(me)) => {
                let admin = me.organization.role == "admin";
                view! {
                <Stack gap="md">
                    <Text size="sm" dimmed=true>
                        {format!("Your role in the organization is {}.", me.organization.role)}
                    </Text>
                    <Panel title="Your boards"
                        caption="The capabilities that you have on each board: from a grant, or \
                                 from the membership of the board.">
                        {if me.capabilities.is_empty() {
                            let message = if admin {
                                "You have no grant on a board. As an organization admin, you can \
                                 change each board."
                            } else {
                                "You have no capability on a board."
                            };
                            view! { <Empty message=message/> }.into_any()
                        } else {
                            view! {
                                <Table label="Your boards">
                                    <thead><tr><th>"Board"</th><th>"Capabilities"</th></tr></thead>
                                    <tbody>
                                        {me.capabilities.into_iter().map(|board| view! {
                                            <TableRow>
                                                <td><Anchor href=format!("/boards/{}", board.board_slug)>
                                                    {board.board_slug.clone()}
                                                </Anchor></td>
                                                <td><Group gap="xs" wrap=true>
                                                    {board.grants.into_iter().map(|g| view! {
                                                        <Pill color=token::TEAL>{g}</Pill>
                                                    }).collect_view()}
                                                    {board.implied.into_iter().map(|g| view! {
                                                        <Pill color=token::MUTED>{g}</Pill>
                                                    }).collect_view()}
                                                </Group></td>
                                            </TableRow>
                                        }).collect_view()}
                                    </tbody>
                                </Table>
                            }.into_any()
                        }}
                    </Panel>
                    {(!me.implicit.is_empty()).then(|| view! {
                        <Panel title="Each member" caption="What each member of the organization can do.">
                            <Group gap="xs" wrap=true>
                                {me.implicit.into_iter().map(|g| view! {
                                    <Pill color=token::MUTED>{g}</Pill>
                                }).collect_view()}
                            </Group>
                        </Panel>
                    })}
                </Stack>
                }
                .into_any()
            }
        }}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_tab_is_the_first_tab() {
        assert_eq!(ProfileTab::from_query(None), ProfileTab::Teams);
        assert_eq!(ProfileTab::from_query(Some("nope")), ProfileTab::Teams);
        for tab in ProfileTab::ALL {
            assert_eq!(ProfileTab::from_query(Some(tab.key())), tab);
        }
        assert_eq!(ProfileTab::AgentKeys.href(), "/me/profile?tab=agent-keys");
    }
}
