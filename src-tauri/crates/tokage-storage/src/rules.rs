//! `command_rules` (docs/08 schema.md §7): local overrides of the command lists. Rules are
//! checked against the pack before they reach this layer (`Policy::validate_rule`).

use rusqlite::{params, Connection, OptionalExtension};
use tokage_core::command_policy::{Level, Rule, RuleSource};

pub fn list(c: &Connection) -> rusqlite::Result<Vec<Rule>> {
    c.prepare("SELECT id, project_id, provider, pattern, level, source FROM command_rules ORDER BY id")?
        .query_map([], |r| {
            let level: String = r.get(4)?;
            let source: String = r.get(5)?;
            Ok((
                Rule {
                    id: Some(r.get(0)?),
                    project_id: r.get(1)?,
                    provider: r.get(2)?,
                    pattern: r.get(3)?,
                    level: Level::ReadOnly,
                    source: RuleSource::User,
                },
                level,
                source,
            ))
        })?
        .filter_map(|row| match row {
            Ok((mut rule, level, source)) => {
                // Rows the CHECK constraints let through always parse; skip anything else.
                rule.level = Level::parse(&level)?;
                rule.source = RuleSource::parse(&source)?;
                Some(Ok(rule))
            }
            Err(e) => Some(Err(e)),
        })
        .collect()
}

/// Inserts the rule, or changes the level of the rule with the same scope and pattern. A
/// `forbidden` rule is never lowered this way: the user deletes it instead, which only
/// removes their own addition (the pack's `forbidden` entries are not rows).
pub fn upsert(c: &Connection, rule: &Rule) -> rusqlite::Result<i64> {
    let existing: Option<(i64, String)> = c
        .query_row(
            "SELECT id, level FROM command_rules
             WHERE IFNULL(project_id, '') = IFNULL(?1, '') AND IFNULL(provider, '') = IFNULL(?2, '')
               AND pattern = ?3",
            params![rule.project_id, rule.provider, rule.pattern],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    match existing {
        Some((id, level)) => {
            if level != Level::Forbidden.as_str() {
                c.execute(
                    "UPDATE command_rules SET level = ?2, source = ?3 WHERE id = ?1",
                    params![id, rule.level.as_str(), rule.source.as_str()],
                )?;
            }
            Ok(id)
        }
        None => {
            c.execute(
                "INSERT INTO command_rules (project_id, provider, pattern, level, source)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    rule.project_id,
                    rule.provider,
                    rule.pattern,
                    rule.level.as_str(),
                    rule.source.as_str()
                ],
            )?;
            Ok(c.last_insert_rowid())
        }
    }
}

pub fn delete(c: &Connection, id: i64) -> rusqlite::Result<()> {
    c.execute("DELETE FROM command_rules WHERE id = ?1", [id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let mut c = Connection::open_in_memory().unwrap();
        c.execute_batch(super::super::PRAGMAS).unwrap();
        super::super::embedded::migrations::runner().run(&mut c).unwrap();
        c.execute("INSERT INTO projects (id, name, local_path) VALUES ('p1', 'a', '/a')", []).unwrap();
        c
    }

    fn rule(project: Option<&str>, pattern: &str, level: Level) -> Rule {
        Rule {
            id: None,
            project_id: project.map(Into::into),
            provider: None,
            pattern: pattern.into(),
            level,
            source: RuleSource::User,
        }
    }

    #[test]
    fn upsert_list_delete() {
        let c = db();
        let a = upsert(&c, &rule(None, "npm test", Level::Project)).unwrap();
        let b = upsert(&c, &rule(Some("p1"), "npm test", Level::Sensitive)).unwrap();
        assert_ne!(a, b);
        // Same scope and pattern: updated in place.
        assert_eq!(upsert(&c, &rule(None, "npm test", Level::ReadOnly)).unwrap(), a);
        let all = list(&c).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].level, Level::ReadOnly);
        assert_eq!(all[1].project_id.as_deref(), Some("p1"));
        delete(&c, a).unwrap();
        assert_eq!(list(&c).unwrap().len(), 1);
        // Project rules go with the project.
        c.execute("DELETE FROM projects WHERE id = 'p1'", []).unwrap();
        assert!(list(&c).unwrap().is_empty());
    }

    #[test]
    fn forbidden_rules_are_not_lowered() {
        let c = db();
        let id = upsert(&c, &rule(None, "terraform", Level::Forbidden)).unwrap();
        assert_eq!(upsert(&c, &rule(None, "terraform", Level::ReadOnly)).unwrap(), id);
        assert_eq!(list(&c).unwrap()[0].level, Level::Forbidden);
    }
}
