use super::*;
use std::collections::BTreeSet;

pub(super) const DOCUMENTS: TableDefinition<&str, &[u8]> = TableDefinition::new("documents");

fn document(
    table: &impl ReadableTable<&'static str, &'static [u8]>,
    id: &str,
) -> Result<wire::KnowledgeDocument> {
    Ok(wire::KnowledgeDocument::decode(
        table.get(id)?.ok_or("文档不存在")?.value(),
    )?)
}
fn summary(mut doc: wire::KnowledgeDocument) -> wire::KnowledgeDocument {
    if let Some((offset, _)) = doc.content.char_indices().nth(300) {
        doc.content.truncate(offset)
    }
    doc
}

impl Workspace {
    pub(super) fn documents(&self, name: &str, bytes: &[u8]) -> Result<Vec<u8>> {
        match name {
            "workbench/knowledge" | "workbench/knowledge-read" | "workbench/knowledge-related" => {
                let q = wire::WorkbenchRequest::decode(bytes)?;
                let table = self.db.begin_read()?.open_table(DOCUMENTS)?;
                if name == "workbench/knowledge-read" {
                    return Ok(document(&table, &q.id)?.encode_to_vec());
                }
                let mut docs = Vec::new();
                if name == "workbench/knowledge-related" {
                    for id in document(&table, &q.id)?.related_ids {
                        docs.push(summary(document(&table, &id)?))
                    }
                    docs.sort_by(|a, b| a.title.cmp(&b.title));
                } else {
                    let query = q.query.to_lowercase();
                    for row in table.iter()?.rev() {
                        let doc = wire::KnowledgeDocument::decode(row?.1.value())?;
                        if query.is_empty()
                            || doc.title.to_lowercase().contains(&query)
                            || doc.content.to_lowercase().contains(&query)
                        {
                            docs.push(summary(doc));
                        }
                        if docs.len() == 200 {
                            break;
                        }
                    }
                }
                Ok(wire::KnowledgeList { documents: docs }.encode_to_vec())
            }
            _ => {
                let tx = self.db.begin_write()?;
                let mut table = tx.open_table(DOCUMENTS)?;
                let (mut doc, expected, remove) = match name {
                    "workbench/knowledge-link" => {
                        let q = wire::KnowledgeLinkRequest::decode(bytes)?;
                        let mut doc = document(&table, &q.id)?;
                        if q.present {
                            doc.related_ids.push(q.target_id)
                        } else {
                            doc.related_ids.retain(|id| id != &q.target_id);
                        }
                        (doc, q.expected_revision, false)
                    }
                    "workbench/knowledge-save" => {
                        let q = wire::KnowledgeSaveRequest::decode(bytes)?;
                        if q.title.trim().is_empty() {
                            return Err("请输入文档标题".into());
                        }
                        (
                            wire::KnowledgeDocument {
                                id: if q.id.is_empty() {
                                    uuid::Uuid::now_v7().to_string()
                                } else {
                                    q.id
                                },
                                title: q.title,
                                content: q.content,
                                related_ids: q.related_ids,
                                ..Default::default()
                            },
                            q.expected_revision,
                            false,
                        )
                    }
                    _ => {
                        let q = wire::KnowledgeDeleteRequest::decode(bytes)?;
                        let mut doc = document(&table, &q.id)?;
                        doc.related_ids.clear();
                        (doc, q.expected_revision, true)
                    }
                };
                let old: wire::KnowledgeDocument = read(&table, &doc.id)?;
                super::records::revision(old.revision, expected)?;
                let old_links: BTreeSet<_> = old.related_ids.into_iter().collect();
                let new_links: BTreeSet<_> =
                    std::mem::take(&mut doc.related_ids).into_iter().collect();
                if new_links.contains(&doc.id) {
                    return Err("文档不能关联自身".into());
                }
                for id in old_links.symmetric_difference(&new_links) {
                    let mut other = document(&table, id)?;
                    other.related_ids.retain(|v| v != &doc.id);
                    if new_links.contains(id) {
                        other.related_ids.push(doc.id.clone());
                        other.related_ids.sort();
                    }
                    other.revision += 1;
                    table.insert(id.as_str(), other.encode_to_vec().as_slice())?;
                }
                doc.related_ids = new_links.into_iter().collect();
                doc.revision = expected + 1;
                let output = if remove {
                    table.remove(doc.id.as_str())?;
                    wire::KnowledgeList::default().encode_to_vec()
                } else {
                    let data = doc.encode_to_vec();
                    table.insert(doc.id.as_str(), data.as_slice())?;
                    data
                };
                drop(table);
                tx.commit()?;
                Ok(output)
            }
        }
    }
}
