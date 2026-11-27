use super::*;
use std::collections::HashSet;

pub(super) const ENABLED: TableDefinition<&str, ()> = TableDefinition::new("enabled_skills");
pub(super) const CUSTOM: TableDefinition<&str, &[u8]> = TableDefinition::new("custom_skills");
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
        let mut skills: Vec<_> = BUILTINS
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
            .collect();
        for entry in self.db.begin_read()?.open_table(CUSTOM)?.iter()? {
            let (_, bytes) = entry?;
            let mut skill = wire::RuntimeSkill::decode(bytes.value())?;
            skill.enabled = enabled.contains(skill.id.as_str());
            skills.push(skill);
        }
        Ok(wire::SkillList { skills })
    }

    pub(super) async fn import_skill(
        &self,
        mut q: wire::SkillImportRequest,
    ) -> Result<wire::RuntimeSkill> {
        if q.content.is_empty() {
            let mut url = reqwest::Url::parse(&q.source)?;
            if !matches!(url.scheme(), "http" | "https") {
                return Err("请输入 HTTP 或 HTTPS 的 Skill 网址".into());
            }
            if url.host_str() == Some("github.com") {
                let parts: Vec<_> = url.path().trim_matches('/').split('/').collect();
                let path = match parts.as_slice() {
                    [owner, repo] => format!("/{owner}/{repo}/HEAD/SKILL.md"),
                    [owner, repo, "blob", rest @ ..] if rest.len() >= 2 => {
                        format!("/{owner}/{repo}/{}", rest.join("/"))
                    }
                    [owner, repo, "tree", branch, rest @ ..] => {
                        format!("/{owner}/{repo}/{branch}/{}/SKILL.md", rest.join("/"))
                            .replace("//", "/")
                    }
                    _ => return Err("请选择 SKILL.md 文件或目录网址".into()),
                };
                url.set_host(Some("raw.githubusercontent.com"))?;
                url.set_path(&path);
                url.set_query(None);
            }
            let mut response = self
                .client("")?
                .get(url)
                .timeout(std::time::Duration::from_secs(30))
                .send()
                .await?
                .error_for_status()?;
            let mut bytes = Vec::new();
            while let Some(part) = response.chunk().await? {
                if part.len() > 1_048_576 - bytes.len() {
                    return Err("Skill正文超过1MB".into());
                }
                bytes.extend_from_slice(&part);
            }
            q.content = String::from_utf8(bytes)?;
        }
        let (id, description) = ai_phone_api::skill::validate_skill(&q.content, &q.name)?;
        if BUILTINS.iter().any(|s| s.id == id) {
            return Err("此名称已用于内置 Skill".into());
        }
        let tx = self.db.begin_write()?;
        let enabled = tx.open_table(ENABLED)?.get(id.as_str())?.is_some();
        let skill = wire::RuntimeSkill {
            name: id.clone(),
            id,
            description,
            enabled,
            content: q.content,
            source: if q.source.is_empty() {
                "local".into()
            } else {
                q.source
            },
            ..Default::default()
        };
        tx.open_table(CUSTOM)?
            .insert(skill.id.as_str(), skill.encode_to_vec().as_slice())?;
        tx.commit()?;
        Ok(skill)
    }

    pub(super) fn update_skill(&self, q: wire::SkillUpdateRequest) -> Result<wire::SkillList> {
        let tx = self.db.begin_write()?;
        {
            let mut enabled = tx.open_table(ENABLED)?;
            let mut custom = tx.open_table(CUSTOM)?;
            if let Some(skill) = BUILTINS.iter().find(|s| s.id == q.id) {
                if q.remove || skill.core {
                    return Err("请选择可启停的 Skill".into());
                }
            } else if q.remove {
                let bytes = custom.remove(q.id.as_str())?.ok_or("找不到 Skill")?;
                tx.open_table(RECORDS)?
                    .insert("last_removed_skill", bytes.value())?;
            } else if custom.get(q.id.as_str())?.is_none() {
                let mut records = tx.open_table(RECORDS)?;
                let bytes = records
                    .remove("last_removed_skill")?
                    .ok_or("找不到 Skill")?;
                if wire::RuntimeSkill::decode(bytes.value())?.id != q.id {
                    return Err("找不到 Skill".into());
                }
                custom.insert(q.id.as_str(), bytes.value())?;
            }
            if q.enabled && !q.remove {
                enabled.insert(q.id.as_str(), ())?;
            } else {
                enabled.remove(q.id.as_str())?;
            }
        }
        tx.commit()?;
        self.skill_list()
    }

    pub(super) fn active_skill_ids(&self) -> Result<Vec<String>> {
        let mut ids: Vec<_> = self.enabled_extras()?.into_iter().collect();
        ids.sort_unstable();
        ids.extend(BUILTINS.iter().filter(|s| s.core).map(|s| s.id.to_owned()));
        Ok(ids)
    }

    pub(super) fn read_active_skill(&self, id: &str) -> Result<String> {
        let read = self.db.begin_read()?;
        let builtin = BUILTINS.iter().find(|s| s.id == id);
        if !builtin.is_some_and(|s| s.core) && read.open_table(ENABLED)?.get(id)?.is_none() {
            return Err("Skill 未启用".into());
        }
        if let Some(skill) = builtin {
            return Ok(skill.body.to_owned());
        }
        let table = read.open_table(CUSTOM)?;
        Ok(wire::RuntimeSkill::decode(table.get(id)?.ok_or("找不到 Skill")?.value())?.content)
    }
}

#[cfg(test)]
#[test]
fn builtins_are_offline_and_selection_survives_reopen() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let store = Workspace::open(dir.path())?;
    assert!(store.active_skill_ids()?.iter().any(|id| id == "draw"));
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
