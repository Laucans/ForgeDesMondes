//! The product's surfaces (`surfaces.json`, contract S), read as its shell
//! reads them.
//!
//! Who a command's caller is on each surface (caller model A): the shell
//! sets the caller of every command a screen sends from the screen's
//! surface, whatever the Micro-UI sent; a test acts as a surface the same
//! way, never with a role written by hand.
//!
//! One source, installed twice by `harness init-repo`: in the units' test
//! kit (`crates/platform/test-kit/src/surfaces.rs`) and in the rules'
//! business kit (`tests/rules/business/src/surfaces.rs`). `std` and
//! `serde_json` only. While the product declares no surfaces, a test acts
//! as the [`FALLBACK_BY`] approver, what both kits sent before surfaces
//! existed.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Value, json};

/// The declaration's file, at the product's root.
pub const FILE: &str = "surfaces.json";

/// Who a test acts as while the product declares no surfaces: its approver.
pub const FALLBACK_BY: &str = "dm";

/// The roles the fallback holds: a critical field's default review role.
pub const FALLBACK_ROLES: [&str; 1] = ["role:approver"];

/// One surface: the caller its screens send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Surface {
    /// The author the caller names.
    pub by: String,
    /// The roles its gestures hold.
    pub roles: Vec<String>,
    /// The permissions its gestures hold.
    pub permissions: Vec<String>,
}

/// The declaration: the surface a test acts as when it names none, and
/// every surface by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Surfaces {
    /// The surface a test acts as when it names none.
    pub default: String,
    /// Every surface, by name.
    pub surfaces: BTreeMap<String, Surface>,
}

/// A command's caller, as the wire carries it (`caller` of a submission),
/// and the surface it was taken from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caller {
    /// The author.
    pub by: String,
    /// The Micro-UI or system that triggers.
    pub from: String,
    /// The roles held.
    pub roles: Vec<String>,
    /// The permissions held.
    pub permissions: Vec<String>,
    /// The surface it acts as; `None` for the fallback, or a screen whose
    /// surface is not declared (fail closed).
    pub surface: Option<String>,
}

impl Caller {
    /// The caller as a submission's body carries it.
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "by": self.by,
            "from": self.from,
            "roles": self.roles,
            "permissions": self.permissions,
        })
    }

    /// The surface it acts as, as a line names it.
    #[must_use]
    pub fn named(&self) -> String {
        self.surface
            .clone()
            .unwrap_or_else(|| "none declared".to_string())
    }
}

/// The strings of a JSON array; an error naming `what` for anything else.
fn strings(value: Option<&Value>, what: &str) -> Result<Vec<String>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    value
        .as_array()
        .ok_or_else(|| format!("{what} is not a list"))?
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_string)
                .ok_or_else(|| format!("{what} holds {item}, not a string"))
        })
        .collect()
}

/// Reads the declaration's text.
///
/// # Errors
///
/// It is not JSON, its `default` names no surface it declares, or a surface
/// has no `by` or a `roles` that is no list of strings.
pub fn parse(text: &str) -> Result<Surfaces, String> {
    let value: Value =
        serde_json::from_str(text).map_err(|e| format!("{FILE} is not JSON: {e}"))?;
    let default = value
        .get("default")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{FILE} names no default surface"))?
        .to_string();
    let mut surfaces = BTreeMap::new();
    let declared = value
        .get("surfaces")
        .and_then(Value::as_object)
        .ok_or_else(|| format!("{FILE} declares no surfaces"))?;
    for (name, surface) in declared {
        let by = surface
            .get("by")
            .and_then(Value::as_str)
            .filter(|by| !by.is_empty())
            .ok_or_else(|| format!("{FILE}: the surface {name} names no `by`"))?
            .to_string();
        let roles = strings(surface.get("roles"), &format!("{FILE}: {name}'s roles"))?;
        let permissions = strings(
            surface.get("permissions"),
            &format!("{FILE}: {name}'s permissions"),
        )?;
        surfaces.insert(
            name.clone(),
            Surface {
                by,
                roles,
                permissions,
            },
        );
    }
    if !surfaces.contains_key(&default) {
        return Err(format!(
            "{FILE}: the default surface {default} is not one it declares"
        ));
    }
    Ok(Surfaces { default, surfaces })
}

/// The product's declaration under `root`: `None` while it declares none.
///
/// # Errors
///
/// The file is there and [`parse`] refuses it.
#[must_use]
pub fn read(root: &Path) -> Option<Result<Surfaces, String>> {
    let text = std::fs::read_to_string(root.join(FILE)).ok()?;
    Some(parse(&text))
}

/// The caller of surface `name` — the default one when `None` — from
/// `from`; the fallback approver while the product declares no surfaces.
///
/// # Errors
///
/// `name` is a surface the declaration does not list, or one is named while
/// the product declares none.
pub fn caller(
    surfaces: Option<&Surfaces>,
    name: Option<&str>,
    from: &str,
) -> Result<Caller, String> {
    let (surfaces, name) = match (surfaces, name) {
        (Some(surfaces), name) => (surfaces, name.unwrap_or(&surfaces.default)),
        (None, Some(name)) => {
            return Err(format!(
                "no {FILE} declares the surface {name}: declare the product's surfaces (contract S)"
            ));
        }
        (None, None) => {
            return Ok(Caller {
                by: FALLBACK_BY.to_string(),
                from: from.to_string(),
                roles: FALLBACK_ROLES.iter().map(ToString::to_string).collect(),
                permissions: Vec::new(),
                surface: None,
            });
        }
    };
    let surface = surfaces.surfaces.get(name).ok_or_else(|| {
        format!(
            "{FILE} declares no surface {name}: it has {}",
            surfaces
                .surfaces
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;
    Ok(Caller {
        by: surface.by.clone(),
        from: from.to_string(),
        roles: surface.roles.clone(),
        permissions: surface.permissions.clone(),
        surface: Some(name.to_string()),
    })
}

/// The caller the shell sets on a screen of surface `surface`: that
/// surface's; a screen that declares none, or one the declaration does not
/// list, acts with no role and no permission — fail closed.
#[must_use]
pub fn on_screen(surfaces: &Surfaces, surface: Option<&str>, from: &str) -> Caller {
    surface
        .and_then(|name| caller(Some(surfaces), Some(name), from).ok())
        .unwrap_or_else(|| Caller {
            by: "anonymous".to_string(),
            from: from.to_string(),
            roles: Vec::new(),
            permissions: Vec::new(),
            surface: None,
        })
}

/// Whether a `POST /commands` answer waits for a review: its result, or
/// the result of a follow-up it submitted (`then[].submitted`), which
/// inherits its leader's caller — `awaiting_review`.
#[must_use]
pub fn parked(answer: &Value) -> bool {
    let waits = |result: Option<&Value>| {
        result
            .and_then(|result| result.get("status"))
            .and_then(Value::as_str)
            == Some("awaiting_review")
    };
    waits(answer.get("result"))
        || answer
            .get("then")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .any(|follow_up| waits(follow_up.get("submitted").and_then(|s| s.get("result"))))
}

/// The review a parked answer waits on, when it names one.
#[must_use]
pub fn review_of(answer: &Value) -> Option<String> {
    let of = |result: Option<&Value>| {
        result
            .and_then(|result| result.get("reviewId"))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    of(answer.get("result")).or_else(|| {
        answer
            .get("then")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .find_map(|follow_up| of(follow_up.get("submitted").and_then(|s| s.get("result"))))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DECLARED: &str = r#"{
      "default": "dm",
      "surfaces": {
        "dm": { "by": "dm", "roles": ["role:approver"], "permissions": ["campaign.write"] },
        "player": { "by": "player", "roles": [] }
      }
    }"#;

    #[test]
    fn a_declaration_reads_every_surface_and_its_default() {
        let surfaces = parse(DECLARED).expect("declared");
        assert_eq!(surfaces.default, "dm");
        assert_eq!(surfaces.surfaces.len(), 2);
        assert_eq!(
            surfaces.surfaces.get("dm"),
            Some(&Surface {
                by: "dm".to_string(),
                roles: vec!["role:approver".to_string()],
                permissions: vec!["campaign.write".to_string()],
            })
        );
        let player = surfaces.surfaces.get("player").expect("player");
        assert!(player.roles.is_empty() && player.permissions.is_empty());
    }

    #[test]
    fn a_broken_declaration_says_what_is_wrong() {
        for (text, said) in [
            ("{", "is not JSON"),
            (
                r#"{"surfaces": {"a": {"by": "a", "roles": []}}}"#,
                "names no default",
            ),
            (r#"{"default": "a"}"#, "declares no surfaces"),
            (
                r#"{"default": "a", "surfaces": {"a": {"roles": []}}}"#,
                "names no `by`",
            ),
            (
                r#"{"default": "a", "surfaces": {"a": {"by": "", "roles": []}}}"#,
                "names no `by`",
            ),
            (
                r#"{"default": "a", "surfaces": {"a": {"by": "a", "roles": "x"}}}"#,
                "roles is not a list",
            ),
            (
                r#"{"default": "a", "surfaces": {"a": {"by": "a", "roles": [1]}}}"#,
                "holds 1, not a string",
            ),
            (
                r#"{"default": "a", "surfaces": {"a": {"by": "a", "roles": [], "permissions": [true]}}}"#,
                "permissions holds true",
            ),
            (
                r#"{"default": "b", "surfaces": {"a": {"by": "a", "roles": []}}}"#,
                "the default surface b is not one it declares",
            ),
        ] {
            let error = parse(text).expect_err(text);
            assert!(error.contains(said), "{text}: {error}");
        }
    }

    #[test]
    fn a_caller_is_its_surface_s_and_the_fallback_approver_without_a_declaration() {
        let surfaces = parse(DECLARED).expect("declared");
        let dm = caller(Some(&surfaces), None, "rules").expect("the default");
        assert_eq!(dm.by, "dm");
        assert_eq!(dm.roles, ["role:approver"]);
        assert_eq!(dm.permissions, ["campaign.write"]);
        assert_eq!(dm.surface.as_deref(), Some("dm"));
        assert_eq!(dm.named(), "dm");
        assert_eq!(
            dm.to_json(),
            json!({ "by": "dm", "from": "rules", "roles": ["role:approver"], "permissions": ["campaign.write"] })
        );
        let player = caller(Some(&surfaces), Some("player"), "rules").expect("player");
        assert_eq!(player.by, "player");
        assert_eq!(player.roles, Vec::<String>::new());
        let unknown = caller(Some(&surfaces), Some("admin"), "rules").expect_err("undeclared");
        assert!(
            unknown.contains("declares no surface admin: it has dm, player"),
            "{unknown}"
        );
        let fallback = caller(None, None, "test").expect("the fallback");
        assert_eq!(fallback.by, FALLBACK_BY);
        assert_eq!(fallback.roles, FALLBACK_ROLES);
        assert_eq!(fallback.from, "test");
        assert_eq!(fallback.surface, None);
        assert_eq!(fallback.named(), "none declared");
        let named = caller(None, Some("player"), "test").expect_err("nothing declared");
        assert!(named.contains("declare the product's surfaces"), "{named}");
    }

    #[test]
    fn a_screen_with_no_declared_surface_acts_with_no_role() {
        let surfaces = parse(DECLARED).expect("declared");
        assert_eq!(
            on_screen(&surfaces, Some("dm"), "rules").roles,
            ["role:approver"]
        );
        for unknown in [None, Some("admin")] {
            let closed = on_screen(&surfaces, unknown, "rules");
            assert!(
                closed.roles.is_empty() && closed.permissions.is_empty(),
                "{unknown:?}"
            );
            assert_eq!(closed.surface, None);
            assert_eq!(closed.from, "rules");
        }
    }

    #[test]
    fn a_review_is_read_on_the_answer_and_on_each_follow_up() {
        let applied = json!({ "result": { "status": "applied" } });
        assert!(!parked(&applied));
        assert_eq!(review_of(&applied), None);
        let waits = json!({ "result": { "status": "awaiting_review", "reviewId": "r1" } });
        assert!(parked(&waits));
        assert_eq!(review_of(&waits).as_deref(), Some("r1"));
        let follow_up = json!({
            "result": { "status": "queued" },
            "then": [
                { "refused": { "error": "x" } },
                { "submitted": { "result": { "status": "awaiting_review", "reviewId": "r2" } } }
            ]
        });
        assert!(parked(&follow_up));
        assert_eq!(review_of(&follow_up).as_deref(), Some("r2"));
        assert!(!parked(
            &json!({ "then": [{ "submitted": { "result": { "status": "applied" } } }] })
        ));
        assert!(!parked(&Value::Null));
    }

    #[test]
    fn the_declaration_is_read_at_a_root_only_when_it_is_there() {
        let root = std::env::temp_dir().join(format!(
            "surfaces-read-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&root).expect("mkdir");
        assert!(read(&root).is_none(), "nothing declared");
        std::fs::write(root.join(FILE), DECLARED).expect("write");
        let declared = read(&root).expect("there").expect("readable");
        std::fs::write(root.join(FILE), "{").expect("write");
        let broken = read(&root).expect("there");
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(declared.default, "dm");
        assert!(broken.is_err());
    }
}
