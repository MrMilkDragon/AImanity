pub fn validate_skill(content: &str, fallback: &str) -> Result<(String, String), String> {
    if content.len() > 1_048_576 || content.contains('\0') {
        return Err("Skill 正文包含无效字符或超过 1 MiB".into());
    }
    let mut lines = content.split_inclusive('\n');
    if lines.next().map(str::trim_end) != Some("---") {
        return Err("SKILL.md 需要 YAML 头部".into());
    }
    let mut header = String::new();
    for line in lines.by_ref() {
        if line.trim_end() == "---" {
            break;
        }
        header.push_str(line);
    }
    if !lines.any(|line| !line.trim().is_empty()) {
        return Err("Skill 需要完整头部和正文".into());
    }
    let fields: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&header).map_err(|e| e.to_string())?;
    let name = fields["name"].as_str().unwrap_or(fallback).trim();
    let description = fields["description"].as_str().unwrap_or_default().trim();
    if name.is_empty() || description.is_empty() {
        return Err("Skill 需要 name 和 description".into());
    }
    if name.chars().count() > 100 {
        return Err("Skill 名称超过 100 个字符".into());
    }
    Ok((name.to_string(), description.chars().take(600).collect()))
}
