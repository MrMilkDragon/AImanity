use super::*;
use std::collections::HashSet;

pub(super) const ENABLED: TableDefinition<&str, ()> = TableDefinition::new("enabled_skills");
struct Skill {
    id: &'static str,
    description: &'static str,
    body: &'static str,
    core: bool,
}
const BUILTINS: &[Skill] = include!(concat!(env!("OUT_DIR"), "/skills.rs"));

impl Workspace {
    fn enabled_extras(&self) -> Result<HashSet<String>> {
        self.db
            .begin_read()?
            .open_table(ENABLED)?
            .iter()?
            .map(|r| Ok(r?.0.value().to_owned()))
            .collect()
    }
    pub(super) fn skill_list(&self) -> Result<wire::SkillList> {
        let enabled = self.enabled_extras()?;
        Ok(wire::SkillList {
            skills: BUILTINS
                .iter()
                .map(|s| wire::RuntimeSkill {
                    id: s.id.into(),
                    name: s.id.into(),
                    description: s.description.into(),
                    content: s.body.into(),
                    source: if s.core { "builtin" } else { "catalog" }.into(),
                    enabled: s.core || enabled.contains(s.id),
                    builtin: true,
                    ..Default::default()
                })
                .collect(),
        })
    }
    pub(super) fn set_builtin_skill(
        &self,
        request: wire::SkillUpdateRequest,
    ) -> Result<wire::SkillList> {
        if request.remove || !BUILTINS.iter().any(|s| s.id == request.id && !s.core) {
            return Err("请选择可启停的内置 Skill".into());
        }
        let tx = self.db.begin_write()?;
        {
            let mut table = tx.open_table(ENABLED)?;
            if request.enabled {
                table.insert(request.id.as_str(), ())?;
            } else {
                table.remove(request.id.as_str())?;
            }
        }
        tx.commit()?;
        self.skill_list()
    }
    pub(super) fn active_skill_ids(&self) -> Result<Vec<&'static str>> {
        let enabled = self.enabled_extras()?;
        Ok(BUILTINS
            .iter()
            .filter(|s| s.core || enabled.contains(s.id))
            .map(|s| s.id)
            .collect())
    }
    pub(super) fn read_active_skill(&self, id: &str) -> Result<&'static str> {
        let skill = BUILTINS.iter().find(|s| s.id == id).ok_or("找不到 Skill")?;
        if !skill.core
            && self
                .db
                .begin_read()?
                .open_table(ENABLED)?
                .get(id)?
                .is_none()
        {
            return Err("Skill 未启用".into());
        }
        Ok(skill.body)
    }
}

#[cfg(test)]
#[test]
fn builtins_are_offline_and_selection_survives_reopen() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let store = Workspace::open(dir.path())?;
    assert!(store.active_skill_ids()?.contains(&"draw"));
    assert!(store.read_active_skill("draw")?.contains("stroke"));
    assert!(store.read_active_skill("hallmark").is_err());
    let toggle = |enabled| {
        wire::SkillUpdateRequest {
            id: "hallmark".into(),
            enabled,
            remove: false,
        }
        .encode_to_vec()
    };
    store.dispatch("workbench/skill-update", &toggle(true))?;
    drop(store);
    let store = Workspace::open(dir.path())?;
    let menu = wire::SkillList::decode(store.dispatch("workbench/skills", &[])?.as_slice())?;
    assert!(menu.skills.iter().any(|s| s.id == "hallmark" && s.enabled));
    assert!(store.read_active_skill("hallmark").is_ok());
    store.dispatch("workbench/skill-update", &toggle(false))?;
    assert!(store.read_active_skill("hallmark").is_err());
    Ok(())
}
