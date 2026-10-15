//! `POST /api/search` and the MCP tool `search` find the work of a person:
//!
//! - `filter.claimed_by` gives the tasks that a person has a claim on, and
//!   `filter.owner` the strategies and the initiatives that a person owns.
//!   Each takes `"me"` (the caller) or the id of a person;
//! - the hits carry the claim, the owner, and the open `blocks` edges
//!   (`blocks_summary`), so that a personal page needs one request;
//! - a value that is not `"me"` and not a UUID is a 400 that names the
//!   field.
//!
//! Runs against the LIVE compose stack (`angreal services up`). Owns the
//! scratch database `kairos_search_people_test`.

mod common;

use std::sync::Arc;

use axum::http::{Method, StatusCode};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use serde_json::{Value, json};
use uuid::Uuid;

use common::{
    AUDIENCE, ISSUER, base_config, drop_scratch_db, recreate_scratch_db, request, spawn_server,
    user_token, with_database,
};
use kairos_db::models::{NewOrganizationMember, OrgRole};
use kairos_db::schema::{organization_members, organizations, users};
use kairos_db::{TenantPool, provision_tenant, run_public_migrations};
use kairos_server::app;
use kairos_server::middleware::auth::Authenticator;

const SCRATCH_DB: &str = "kairos_search_people_test";
const TENANT: [(&str, &str); 1] = [("x-tenant", "acme")];

fn codes(body: &Value, group: &str) -> Vec<String> {
    let mut codes: Vec<String> = body["results"][group]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|i| i["short_code"].as_str().expect("code").to_string())
                .collect()
        })
        .unwrap_or_default();
    codes.sort();
    codes
}

/// The text of one MCP `search` call, over a new session.
async fn mcp_search(base_url: &str, token: &str, filter: Value) -> String {
    let http = reqwest::Client::new();
    let url = format!("{base_url}/mcp");
    let post = |body: Value, session: Option<String>| {
        let mut request = http
            .post(&url)
            .bearer_auth(token)
            .header("x-tenant", "acme")
            .header("accept", "application/json, text/event-stream")
            .json(&body);
        if let Some(session) = session {
            request = request.header("mcp-session-id", session);
        }
        request.send()
    };
    let response = post(
        json!({
            "jsonrpc": "2.0", "id": 0, "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "kairos-search-people", "version": "0.0.0"},
            },
        }),
        None,
    )
    .await
    .expect("initialize");
    let session = response
        .headers()
        .get("mcp-session-id")
        .map(|v| v.to_str().expect("session id").to_string());
    post(
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        session.clone(),
    )
    .await
    .expect("initialized");
    let body = post(
        json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": "search", "arguments": {"filter": filter}},
        }),
        session,
    )
    .await
    .expect("tools/call")
    .text()
    .await
    .expect("MCP body");
    let message: Value = serde_json::from_str(&body).unwrap_or_else(|_| {
        body.lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .filter_map(|data| serde_json::from_str::<Value>(data.trim()).ok())
            .find(|value| value.get("jsonrpc").is_some())
            .unwrap_or_else(|| panic!("no JSON-RPC message in {body:?}"))
    });
    message["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("search gave no text: {message}"))
        .to_string()
}

#[tokio::test]
async fn search_finds_the_work_of_a_person_against_live_stack() {
    let mut admin_conn = recreate_scratch_db(SCRATCH_DB);
    let scratch_url = with_database(&common::admin_database_url(), SCRATCH_DB);
    let mut conn = PgConnection::establish(&scratch_url).expect("connecting to scratch database");
    run_public_migrations(&mut conn).expect("running public migrations");
    provision_tenant(&mut conn, "acme", "Acme Inc").expect("provisioning acme");
    let org_id: Uuid = organizations::table
        .filter(organizations::slug.eq("acme"))
        .select(organizations::id)
        .first(&mut conn)
        .expect("acme org row");
    let http = reqwest::Client::new();
    let svc = user_token(&http, "svc").await;
    let alice = user_token(&http, "alice").await;
    let bob = user_token(&http, "bob").await;
    let pool = TenantPool::new(&scratch_url, 4).await.expect("pool");
    let auth = Arc::new(
        Authenticator::discover(ISSUER, AUDIENCE)
            .await
            .expect("OIDC discovery against live Dex"),
    );
    let router = app::router(app::state_with(base_config(&scratch_url), pool, auth));
    let as_user = |token: &str, method: Method, uri: &str, body: Option<Value>| {
        let (router, token, uri) = (router.clone(), token.to_string(), uri.to_string());
        async move { request(&router, method, &uri, Some(&token), &TENANT, body).await }
    };

    let mut ids = Vec::new();
    for (token, email, role) in [
        (&svc, "svc@kairos.test", OrgRole::Admin),
        (&alice, "alice@kairos.test", OrgRole::Member),
        (&bob, "bob@kairos.test", OrgRole::Member),
    ] {
        let _ = as_user(token, Method::GET, "/api/whoami", None).await;
        let id: Uuid = users::table
            .filter(users::email.eq(email))
            .select(users::id)
            .first(&mut conn)
            .expect("user provisioned");
        diesel::insert_into(organization_members::table)
            .values(NewOrganizationMember {
                organization_id: org_id,
                user_id: id,
                role,
            })
            .execute(&mut conn)
            .expect("granting membership");
        ids.push(id);
    }
    let (alice_id, bob_id) = (ids[1], ids[2]);

    // A team with alice and bob, and its delivery board.
    let (status, team) = as_user(
        &svc,
        Method::POST,
        "/api/teams",
        Some(json!({"name": "Web", "slug": "web", "code_prefix": "WEB"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{team}");
    let team_id = team["id"].as_str().expect("team id").to_string();
    for user in [alice_id, bob_id] {
        let (status, body) = as_user(
            &svc,
            Method::POST,
            &format!("/api/teams/{team_id}/members"),
            Some(json!({"user_id": user})),
        )
        .await;
        assert!(status.is_success(), "{status}: {body}");
    }
    let board = team["delivery_board_id"]
        .as_str()
        .expect("delivery board")
        .to_string();
    let (status, detail) = as_user(&svc, Method::GET, &format!("/api/boards/{board}"), None).await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    let column = |name: &str| -> String {
        detail["columns"]
            .as_array()
            .expect("columns")
            .iter()
            .find(|c| c["name"] == name)
            .and_then(|c| c["id"].as_str())
            .unwrap_or_else(|| panic!("no column {name}"))
            .to_string()
    };

    // alice holds one task, bob holds one, and one task has no claim.
    let task = |title: &'static str| {
        let as_user = &as_user;
        let (board, alice) = (board.clone(), alice.clone());
        async move {
            let (status, body) = as_user(
                &alice,
                Method::POST,
                "/api/tasks",
                Some(json!({"board_id": board, "title": title})),
            )
            .await;
            assert_eq!(status, StatusCode::CREATED, "{body}");
            body["short_code"].as_str().expect("code").to_string()
        }
    };
    let mine = task("Mine").await;
    let bobs = task("Bob's").await;
    let free = task("Free").await;
    for (token, code) in [(&alice, &mine), (&bob, &bobs)] {
        for name in ["Todo", "Active"] {
            let (status, body) = as_user(
                token,
                Method::POST,
                &format!("/api/tasks/{code}/transition"),
                Some(json!({"to_column_id": column(name)})),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{body}");
        }
    }
    // The free task blocks alice's task.
    let (status, body) = as_user(
        &alice,
        Method::POST,
        "/api/relationships",
        Some(json!({
            "source_short_code": free,
            "target_short_code": mine,
            "relationship": "blocks",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    // alice owns one initiative; a second initiative has no owner.
    let (status, initiatives) = as_user(&svc, Method::GET, "/api/boards/initiatives", None).await;
    assert_eq!(status, StatusCode::OK, "{initiatives}");
    let initiative_board = initiatives["id"].as_str().expect("board id").to_string();
    let (status, body) = as_user(
        &svc,
        Method::POST,
        &format!("/api/boards/{initiative_board}/members"),
        Some(json!({"user_id": alice_id, "capabilities": ["transition_items"]})),
    )
    .await;
    assert!(status.is_success(), "{status}: {body}");
    let mut owned = Vec::new();
    for title in ["Owned", "Unowned"] {
        let (status, body) = as_user(
            &svc,
            Method::POST,
            "/api/initiatives",
            Some(json!({"board_id": "initiatives", "title": title})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        owned.push(body["short_code"].as_str().expect("code").to_string());
    }
    let (status, body) = as_user(
        &alice,
        Method::POST,
        &format!("/api/initiatives/{}/owner/claim", owned[0]),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let search = |token: &str, filter: Value| {
        let token = token.to_string();
        let as_user = &as_user;
        async move {
            as_user(
                &token,
                Method::POST,
                "/api/search",
                Some(json!({"filter": filter})),
            )
            .await
        }
    };

    // --- claimed_by: "me" is the caller; a UUID is that person.
    let (status, body) = search(&alice, json!({"claimed_by": "me"})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(codes(&body, "tasks"), vec![mine.clone()], "{body}");
    assert!(
        body["results"].get("initiatives").is_none(),
        "only tasks: {body}"
    );
    let hit = &body["results"]["tasks"][0];
    assert_eq!(
        hit["claim"]["user_id"],
        json!(alice_id.to_string()),
        "{body}"
    );
    assert_eq!(
        body["blocks_summary"][&mine],
        json!({"blocked_by": 1, "blocks": 0}),
        "{body}"
    );
    let (status, body) = search(&alice, json!({"claimed_by": bob_id.to_string()})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(codes(&body, "tasks"), vec![bobs.clone()], "{body}");
    let (status, body) = search(&bob, json!({"claimed_by": "ME"})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(codes(&body, "tasks"), vec![bobs.clone()], "{body}");

    // --- owner: only strategies and initiatives, with the owner on the hit.
    let (status, body) = search(&alice, json!({"owner": "me"})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        codes(&body, "initiatives"),
        vec![owned[0].clone()],
        "{body}"
    );
    assert!(body["results"].get("tasks").is_none(), "only owned: {body}");
    assert_eq!(
        body["results"]["initiatives"][0]["owner"]["user_id"],
        json!(alice_id.to_string()),
        "{body}"
    );
    let (status, body) = search(&bob, json!({"owner": "me"})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["total"], 0, "bob owns nothing: {body}");

    // --- A value that is not "me" and not a UUID names the field.
    for field in ["claimed_by", "owner"] {
        let (status, body) = search(&alice, json!({ field: "alice" })).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(
            body["error"]["details"]["field"],
            json!(format!("filter.{field}")),
            "{body}"
        );
    }

    // --- MCP: the same filter, the same person.
    let server = spawn_server(router.clone()).await;
    let text = mcp_search(&server.base_url, &alice, json!({"claimed_by": "me"})).await;
    assert!(text.contains(&mine), "{text}");
    assert!(!text.contains(&bobs), "{text}");
    let text = mcp_search(&server.base_url, &alice, json!({"owner": "me"})).await;
    assert!(text.contains(&owned[0]), "{text}");
    assert!(!text.contains(&owned[1]), "{text}");

    drop(conn);
    drop_scratch_db(&mut admin_conn, SCRATCH_DB);
}
