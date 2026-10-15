//! The data of the personal pages: two searches (the tasks that the person
//! has a claim on, and the strategies and initiatives that the person owns),
//! the unread notifications, and the rule that orders "Needs you".

use std::collections::BTreeMap;

use aurora_dark::tokens::ApiError;
use serde::Deserialize;
use serde_json::json;

use crate::auth::Auth;

/// A claim with no change for this many days is stale.
pub const STALE_DAYS: f64 = 5.0;

/// The most items that one search of the page reads. A person with more
/// claims than this has a larger problem than the page.
const LIMIT: i64 = 100;

/// mirror of: `kairos_client::types_search::SearchResponse` (partial).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct Work {
    pub results: WorkGroups,
    /// The count of all the matches, also the ones after [`LIMIT`].
    #[serde(default)]
    pub total: i64,
    #[serde(default)]
    pub blocks_summary: BTreeMap<String, BlocksCounts>,
}

/// mirror of: `kairos_client::types_search::SearchResultGroups` (partial).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct WorkGroups {
    #[serde(default)]
    pub strategies: Vec<WorkItem>,
    #[serde(default)]
    pub initiatives: Vec<WorkItem>,
    #[serde(default)]
    pub tasks: Vec<WorkItem>,
}

/// mirror of: `kairos_client::types::{Strategy, Initiative, Task}`
/// (partial): the fields that the two types share, and the claim of a task.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct WorkItem {
    pub short_code: String,
    pub title: String,
    pub board_id: String,
    pub column_id: String,
    pub updated_at: String,
    #[serde(default)]
    pub task_type: Option<String>,
    #[serde(default)]
    pub claim: Option<Claim>,
}

/// mirror of: `kairos_client::types::TaskClaim` (partial).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Claim {
    pub claimed_at: String,
    #[serde(default)]
    pub agent: bool,
}

/// mirror of: `kairos_client::types_org::BlocksCounts`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
pub struct BlocksCounts {
    pub blocked_by: i64,
    pub blocks: i64,
}

impl Work {
    /// The words that say the page shows part of the list, or `None` when
    /// it shows all of it. Pure, host-tested.
    pub fn more_than_shown(&self) -> Option<String> {
        let shown = (self.results.strategies.len()
            + self.results.initiatives.len()
            + self.results.tasks.len()) as i64;
        (self.total > shown).then(|| {
            format!(
                "The list shows {shown} of {}: the oldest changes first.",
                self.total
            )
        })
    }

    /// The open blockers of an item: 0 when it has none.
    pub fn blocked_by(&self, code: &str) -> i64 {
        self.blocks_summary
            .get(code)
            .map_or(0, |counts| counts.blocked_by)
    }
}

/// The body of a search for the work of the caller. `field` is
/// `claimed_by` or `owner`. Pure, host-tested.
fn body(field: &str) -> serde_json::Value {
    json!({
        "filter": { field: "me" },
        "sort": { "field": "updated_at", "order": "asc" },
        "limit": LIMIT,
    })
}

/// The tasks that the caller has a claim on, the oldest change first.
pub async fn my_claims(auth: Auth) -> Result<Work, ApiError> {
    crate::api::post_json(auth, "/api/search", &body("claimed_by")).await
}

/// The strategies and the initiatives that the caller owns.
pub async fn my_owned(auth: Auth) -> Result<Work, ApiError> {
    crate::api::post_json(auth, "/api/search", &body("owner")).await
}

/// Why a row is in "Needs you", the most urgent first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Reason {
    /// A claimed task has an open blocker.
    Blocked,
    /// A notification is not read.
    Unread,
    /// A claimed task has no change for [`STALE_DAYS`] days.
    Stale,
}

/// One row of "Needs you".
#[derive(Clone, Debug, PartialEq)]
pub struct Need {
    pub reason: Reason,
    pub short_code: Option<String>,
    pub text: String,
    /// The time of the row (RFC 3339): the last change of the task, or the
    /// time of the notification.
    pub at: String,
}

/// An unread notification, as "Needs you" reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct Unread {
    pub short_code: Option<String>,
    pub text: String,
    pub created_at: String,
}

/// Is `updated_at` before `cutoff`? Both are UTC RFC 3339 times, so the
/// first 19 characters (to the second) compare as text. Pure, host-tested.
pub fn is_before(updated_at: &str, cutoff: &str) -> bool {
    let head = |value: &str| value.get(..19).unwrap_or(value).to_string();
    head(updated_at) < head(cutoff)
}

/// The rows of "Needs you": the blocked claims, then the unread
/// notifications, then the stale claims. A blocked claim does not show
/// again as stale. `cutoff` is now minus [`STALE_DAYS`]. Pure, host-tested.
pub fn needs_you(claims: &Work, unread: &[Unread], cutoff: &str) -> Vec<Need> {
    let mut rows = Vec::new();
    for task in &claims.results.tasks {
        let blockers = claims.blocked_by(&task.short_code);
        if blockers > 0 {
            let noun = if blockers == 1 { "blocker" } else { "blockers" };
            rows.push(Need {
                reason: Reason::Blocked,
                short_code: Some(task.short_code.clone()),
                text: format!("{} waits on {blockers} open {noun}.", task.title),
                at: task.updated_at.clone(),
            });
        } else if is_before(&task.updated_at, cutoff) {
            rows.push(Need {
                reason: Reason::Stale,
                short_code: Some(task.short_code.clone()),
                text: format!(
                    "{} has no change for {STALE_DAYS} days or more.",
                    task.title
                ),
                at: task.updated_at.clone(),
            });
        }
    }
    rows.extend(unread.iter().map(|n| Need {
        reason: Reason::Unread,
        short_code: n.short_code.clone(),
        text: n.text.clone(),
        at: n.created_at.clone(),
    }));
    rows.sort_by(|a, b| a.reason.cmp(&b.reason));
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(code: &str, updated_at: &str) -> WorkItem {
        WorkItem {
            short_code: code.into(),
            title: format!("Title of {code}"),
            board_id: "b".into(),
            column_id: "c".into(),
            updated_at: updated_at.into(),
            task_type: Some("task".into()),
            claim: None,
        }
    }

    #[test]
    fn the_search_asks_for_the_caller() {
        assert_eq!(
            body("claimed_by"),
            json!({
                "filter": {"claimed_by": "me"},
                "sort": {"field": "updated_at", "order": "asc"},
                "limit": 100,
            })
        );
        assert_eq!(body("owner")["filter"], json!({"owner": "me"}));
    }

    #[test]
    fn the_mirror_decodes_the_server_shape() {
        let body = json!({
            "results": {"tasks": [{
                "id": "1", "short_code": "DEMO-T-0001", "title": "One",
                "content": "", "board_id": "b", "column_id": "c",
                "task_type": "bug", "work_class": "planned", "version": 1,
                "created_by": "u", "updated_by": "u",
                "created_at": "2026-10-01T10:00:00Z", "updated_at": "2026-10-02T10:00:00Z",
                "claim": {"user_id": "u", "display_name": "Alice", "agent": true,
                          "claimed_at": "2026-10-02T10:00:00Z"},
            }]},
            "total": 1, "limit": 100, "offset": 0,
            "blocks_summary": {"DEMO-T-0001": {"blocked_by": 2, "blocks": 0}},
        });
        let work: Work = serde_json::from_value(body).expect("decodes");
        assert_eq!(work.results.tasks.len(), 1);
        assert!(
            work.results.tasks[0]
                .claim
                .as_ref()
                .is_some_and(|c| c.agent)
        );
        assert_eq!(work.blocked_by("DEMO-T-0001"), 2);
        assert_eq!(work.blocked_by("DEMO-T-0002"), 0);
    }

    #[test]
    fn a_long_list_says_that_it_shows_a_part() {
        let mut work = Work::default();
        work.results.tasks = vec![task("DEMO-T-0001", "2026-10-01T00:00:00Z")];
        work.total = 1;
        assert_eq!(work.more_than_shown(), None);
        work.total = 140;
        assert_eq!(
            work.more_than_shown().as_deref(),
            Some("The list shows 1 of 140: the oldest changes first.")
        );
    }

    #[test]
    fn times_compare_to_the_second() {
        assert!(is_before(
            "2026-10-01T10:00:00.123456Z",
            "2026-10-09T00:00:00.000Z"
        ));
        assert!(!is_before(
            "2026-10-09T00:00:00Z",
            "2026-10-09T00:00:00.000Z"
        ));
        assert!(!is_before(
            "2026-10-10T00:00:00Z",
            "2026-10-09T00:00:00.000Z"
        ));
    }

    #[test]
    fn blocked_comes_first_then_unread_then_stale() {
        let cutoff = "2026-10-09T00:00:00.000Z";
        let mut claims = Work::default();
        claims.results.tasks = vec![
            task("DEMO-T-0001", "2026-10-01T00:00:00Z"), // stale
            task("DEMO-T-0002", "2026-10-01T00:00:00Z"), // blocked and old
            task("DEMO-T-0003", "2026-10-12T00:00:00Z"), // fresh
        ];
        claims.blocks_summary.insert(
            "DEMO-T-0002".into(),
            BlocksCounts {
                blocked_by: 1,
                blocks: 0,
            },
        );
        let unread = vec![Unread {
            short_code: Some("DEMO-T-0009".into()),
            text: "Bob gave you DEMO-T-0009.".into(),
            created_at: "2026-10-13T00:00:00Z".into(),
        }];
        let rows = needs_you(&claims, &unread, cutoff);
        let seen: Vec<(Reason, Option<&str>)> = rows
            .iter()
            .map(|r| (r.reason, r.short_code.as_deref()))
            .collect();
        assert_eq!(
            seen,
            vec![
                (Reason::Blocked, Some("DEMO-T-0002")),
                (Reason::Unread, Some("DEMO-T-0009")),
                (Reason::Stale, Some("DEMO-T-0001")),
            ]
        );
        assert_eq!(
            rows[0].text,
            "Title of DEMO-T-0002 waits on 1 open blocker."
        );
    }

    #[test]
    fn nothing_needs_a_person_with_fresh_work() {
        let mut claims = Work::default();
        claims.results.tasks = vec![task("DEMO-T-0001", "2026-10-12T00:00:00Z")];
        assert!(needs_you(&claims, &[], "2026-10-09T00:00:00.000Z").is_empty());
    }
}
