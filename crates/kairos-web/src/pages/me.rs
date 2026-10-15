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

use std::collections::HashMap;

use aurora_dark::components::{
    Anchor, Empty, ErrorState, Group, Loading, Panel, Pill, RelativeTime, SimpleGrid, Stack, Table,
    TableRow, Text,
};
use aurora_dark::frame::{Card, PageHeader, TabItem, Tabs};
use aurora_dark::tokens::{ApiError, token};
use futures_util::future::join_all;
use leptos::prelude::*;
use leptos_router::hooks::use_query_map;

use crate::api;
use crate::auth::{Auth, use_auth};
use crate::pages::boards::data::{BoardDetail, board_path, list_boards};
use crate::pages::teams::api::{list_teams, team_announcements};

pub(crate) mod data;

use data::{Need, Reason, STALE_DAYS, Unread, Work, WorkItem};

/// The work of the person, and the names that the page shows for it.
#[derive(Clone, Debug, Default)]
struct MyWork {
    claims: Work,
    owned: Work,
    /// The board of each id: (slug, name).
    boards: HashMap<String, (String, String)>,
    /// The name of each column of the boards of the owned items.
    columns: HashMap<String, String>,
}

impl MyWork {
    fn board(&self, id: &str) -> (String, String) {
        self.boards
            .get(id)
            .cloned()
            .unwrap_or_else(|| (id.to_string(), "Board".to_string()))
    }
}

/// The work of the person. The two searches must answer. The names of the
/// boards and of the columns are only labels: when they do not load, the
/// page shows the work with the fallback labels.
async fn load_work(auth: Auth) -> Result<MyWork, ApiError> {
    let (claims, owned, boards) = futures_util::join!(
        data::my_claims(auth),
        data::my_owned(auth),
        list_boards(auth)
    );
    let (claims, owned) = (claims?, owned?);
    let boards = boards.unwrap_or_default();
    let mut owned_boards: Vec<&str> = owned
        .results
        .strategies
        .iter()
        .chain(owned.results.initiatives.iter())
        .map(|item| item.board_id.as_str())
        .collect();
    owned_boards.sort_unstable();
    owned_boards.dedup();
    let paths: Vec<String> = owned_boards.iter().map(|id| board_path(id)).collect();
    let details = join_all(
        paths
            .iter()
            .map(|path| api::get_json::<BoardDetail>(auth, path)),
    )
    .await;
    let mut columns = HashMap::new();
    for detail in details.into_iter().flatten() {
        for column in detail.columns {
            columns.insert(column.id, column.name);
        }
    }
    Ok(MyWork {
        claims,
        owned,
        boards: boards
            .into_iter()
            .map(|b| (b.id, (b.slug, b.name)))
            .collect(),
        columns,
    })
}

async fn load_unread(auth: Auth) -> Result<Vec<Unread>, ApiError> {
    let list = crate::pages::notifications::list(auth).await?;
    Ok(list
        .items
        .into_iter()
        .filter(|n| n.read_at.is_none())
        .map(|n| Unread {
            short_code: n.short_code,
            text: n.text,
            created_at: n.created_at,
        })
        .collect())
}

/// Now minus [`STALE_DAYS`], as an RFC 3339 time.
fn stale_cutoff() -> String {
    let at = js_sys::Date::now() - STALE_DAYS * 24.0 * 60.0 * 60.0 * 1000.0;
    js_sys::Date::new(&at.into())
        .to_iso_string()
        .as_string()
        .unwrap_or_default()
}

/// `/me`: the work of the person.
#[component]
pub fn MyWorkPage() -> impl IntoView {
    let auth = use_auth();
    let whoami = use_context::<LocalResource<Result<api::Whoami, ApiError>>>();
    let reload = RwSignal::new(0u32);
    let work = LocalResource::new(move || {
        let _ = auth.token();
        reload.get();
        load_work(auth)
    });
    let unread = LocalResource::new(move || {
        let _ = auth.token();
        reload.get();
        load_unread(auth)
    });
    let retry = Callback::new(move |_| reload.update(|n| *n += 1));
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
                let needs = data::needs_you(&work.claims, &unread, &stale_cutoff());
                let claims = work.clone();
                view! {
                    <Stack gap="md">
                        <Panel title="Needs you"
                            caption=format!("Blocked claims first, then unread notifications, \
                                             then claims with no change for {STALE_DAYS} days.")>
                            <NeedsTable needs/>
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

/// The words and the color of a reason.
fn reason_pill(reason: Reason) -> (&'static str, &'static str) {
    match reason {
        Reason::Blocked => ("Blocked", token::BAD),
        Reason::Unread => ("Unread", token::ICE),
        Reason::Stale => ("No change", token::GOLD),
    }
}

#[component]
fn NeedsTable(needs: Vec<Need>) -> impl IntoView {
    if needs.is_empty() {
        return view! { <Empty message="Nothing needs you now."/> }.into_any();
    }
    view! {
        <div data-testid="needs-you">
            <Table label="Needs you">
                <thead><tr><th>"Why"</th><th>"Item"</th><th>"What"</th><th>"When"</th></tr></thead>
                <tbody>
                    {needs.into_iter().map(|need| {
                        let (label, color) = reason_pill(need.reason);
                        view! {
                            <TableRow>
                                <td><Pill color=color>{label}</Pill></td>
                                <td>{need.short_code.map(|code| view! {
                                    <Anchor href=format!("/items/{code}")>{code.clone()}</Anchor>
                                })}</td>
                                <td><Text size="sm">{need.text}</Text></td>
                                <td><RelativeTime iso=need.at/></td>
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
                        let (slug, name) = work.board(&task.board_id);
                        let blockers = work.claims.blocked_by(&task.short_code);
                        let claim = task.claim.clone();
                        view! {
                            <TableRow>
                                <td><ItemCell item=task.clone()/></td>
                                <td><Anchor href=format!("/boards/{slug}")>{name}</Anchor></td>
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
                        let (slug, name) = work.board(&item.board_id);
                        let column = work.columns.get(&item.column_id).cloned().unwrap_or_default();
                        view! {
                            <TableRow>
                                <td><ItemCell item=item.clone()/></td>
                                <td><Pill color=token::VIOLET>{kind}</Pill></td>
                                <td><Anchor href=format!("/boards/{slug}")>{name}</Anchor></td>
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

/// The tabs of the profile: (value of `?tab=`, label).
const TABS: [(&str, &str); 4] = [
    ("teams", "Teams"),
    ("access", "Access"),
    ("notifications", "Notifications"),
    ("agent-keys", "Agent keys"),
];

/// The tab of a `?tab=` value: the first tab for no value or an unknown
/// one. Pure, host-tested.
fn tab_of(value: Option<&str>) -> &'static str {
    value
        .and_then(|v| TABS.iter().find(|(key, _)| *key == v))
        .map_or(TABS[0].0, |(key, _)| key)
}

/// `/me/profile`: the person and the settings.
#[component]
pub fn ProfilePage() -> impl IntoView {
    let whoami = use_context::<LocalResource<Result<api::Whoami, ApiError>>>();
    let query = use_query_map();
    let tab = Memo::new(move |_| tab_of(query.read().get("tab").as_deref()).to_string());
    let tab_value = RwSignal::new(tab.get_untracked());
    Effect::new(move |_| tab_value.set(tab.get()));
    let tabs: Vec<TabItem> = TABS
        .iter()
        .map(|(key, label)| TabItem::new(*key, *label).href(format!("/me/profile?tab={key}")))
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
            {move || match tab.get().as_str() {
                "access" => view! { <AccessTab/> }.into_any(),
                "notifications" => view! { <crate::pages::notifications::NotificationSettingsPanel/> }.into_any(),
                "agent-keys" => view! { <crate::pages::agent_keys::AgentKeysPanel/> }.into_any(),
                _ => view! { <TeamsTab/> }.into_any(),
            }}
        </div>
    }
}

/// One team of the person, with its board and its latest announcement.
#[derive(Clone, Debug)]
struct MyTeam {
    slug: String,
    name: String,
    board: Option<String>,
    announcement: Option<String>,
}

async fn load_teams(auth: Auth, mine: Vec<api::WhoamiTeam>) -> Result<Vec<MyTeam>, ApiError> {
    let teams = list_teams(auth).await?;
    let boards: HashMap<String, Option<String>> = teams
        .into_iter()
        .map(|t| (t.id, t.delivery_board_id))
        .collect();
    let announcements = join_all(mine.iter().map(|team| team_announcements(auth, &team.id))).await;
    let mut out = Vec::new();
    for (team, announcements) in mine.into_iter().zip(announcements) {
        let announcements = announcements?;
        out.push(MyTeam {
            board: boards.get(&team.id).cloned().flatten(),
            // The server sends the pinned first, then the newest.
            announcement: announcements.first().map(|a| a.body.clone()),
            slug: team.slug,
            name: team.name,
        });
    }
    Ok(out)
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
        load_teams(auth, mine)
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
        assert_eq!(tab_of(None), "teams");
        assert_eq!(tab_of(Some("access")), "access");
        assert_eq!(tab_of(Some("agent-keys")), "agent-keys");
        assert_eq!(tab_of(Some("nope")), "teams");
    }
}
