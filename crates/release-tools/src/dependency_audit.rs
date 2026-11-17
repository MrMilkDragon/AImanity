use crate::Result;
use regex::Regex;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs::File,
    io::{self, Write},
    path::Path,
    process::Command,
};

const TARGETS: [&str; 4] = [
    "aarch64-apple-darwin",
    "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-gnu",
    "x86_64-pc-windows-msvc",
];

fn graph(manifest: &str, target: &str) -> Result<BTreeSet<String>> {
    let output = Command::new("cargo")
        .args([
            "tree",
            "--manifest-path",
            manifest,
            "--locked",
            "--target",
            target,
            "--prefix",
            "none",
            "--format",
            "{p}",
        ])
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "Cargo dependency graph for {target}: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(std::str::from_utf8(&output.stdout)?
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_ascii_whitespace();
            let (name, version) = (fields.next()?, fields.next()?.strip_prefix('v')?);
            Some(format!("{name}@{version}"))
        })
        .collect())
}

pub(super) fn run(arguments: &[OsString]) -> Result<()> {
    let (file, target) = match arguments {
        [option, file] if option == "--output" => (file, None),
        [select, target, option, file] | [option, file, select, target]
            if select == "--target" && option == "--output" =>
        {
            let target = target
                .to_str()
                .filter(|target| TARGETS.contains(target))
                .ok_or("Unknown dependency audit target")?;
            (file, Some(target))
        }
        _ => {
            return Err(
                "audit-dependencies requires --output FILE and accepts --target TRIPLE".into(),
            );
        }
    };
    let package_pattern = Regex::new(r"^[a-z0-9._-]+@[0-9][a-zA-Z0-9.+_-]*$")?;
    let advisory_pattern =
        Regex::new(r"^(?:RUSTSEC-[0-9]{4}-[0-9]{4}|GHSA-[a-z0-9]{4}-[a-z0-9]{4}-[a-z0-9]{4})$")?;
    let mut rust = BTreeMap::new();
    let mut blocked: BTreeMap<_, Vec<String>> = TARGETS
        .into_iter()
        .map(|target| (target, Vec::new()))
        .collect();
    for (name, manifest, lock) in [
        ("server", "Cargo.toml", "Cargo.lock"),
        ("client", "apps/native/Cargo.toml", "apps/native/Cargo.lock"),
    ] {
        let output = Command::new("cargo")
            .args(["audit", "--file", lock, "--json"])
            .output()?;
        if !matches!(output.status.code(), Some(0 | 1)) {
            return Err(format!(
                "Dependency audit did not complete: {}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        let report: Value = serde_json::from_slice(&output.stdout)?;
        let database = report["database"]["last-commit"]
            .as_str()
            .ok_or("Advisory database missing")?;
        let vulnerabilities = report["vulnerabilities"]["list"]
            .as_array()
            .ok_or("Vulnerability result missing")?;
        let reachable = TARGETS
            .into_iter()
            .filter(|target| name == "server" || !target.contains("linux"))
            .map(|target| Ok((target, graph(manifest, target)?)))
            .collect::<Result<Vec<_>>>()?;
        let warnings = report["warnings"]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(category, rows)| Some((category.as_str(), rows.as_array()?)));
        let mut findings = Vec::new();
        for (category, rows) in std::iter::once(("vulnerability", vulnerabilities)).chain(warnings)
        {
            let blocking = matches!(category, "vulnerability" | "unsound");
            let category = match category {
                "vulnerability" | "unsound" | "unmaintained" | "yanked" | "notice" => category,
                _ => "unknown",
            };
            for row in rows {
                let package = format!(
                    "{}@{}",
                    row["package"]["name"].as_str().unwrap_or_default(),
                    row["package"]["version"].as_str().unwrap_or_default()
                );
                let advisory = row["advisory"]["id"]
                    .as_str()
                    .filter(|id| advisory_pattern.is_match(id))
                    .unwrap_or("unknown");
                let active: Vec<_> = reachable
                    .iter()
                    .filter(|(_, graph)| graph.contains(&package))
                    .map(|(target, _)| *target)
                    .collect();
                if blocking {
                    for target in &active {
                        blocked
                            .get_mut(target)
                            .expect("configured target")
                            .push(format!("{name}:{advisory}"));
                    }
                }
                findings.push(json!({
                    "package": if package.len() <= 256 && package_pattern.is_match(&package) { package.as_str() } else { "unknown" },
                    "advisory": advisory, "category": category, "active_targets": active, "blocking_when_active": blocking,
                }));
            }
        }
        let mut hash = Sha256::new();
        io::copy(&mut File::open(lock)?, &mut hash)?;
        rust.insert(
            name,
            json!({
                "lock_sha256": format!("{:x}", hash.finalize()),
                "database_commit": database, "findings": findings,
            }),
        );
    }
    let report = json!({"format":3,"completed":true,"blocked_targets":blocked,"rust":rust,"blanket_ignores":[]});
    let file = Path::new(file);
    if let Some(parent) = file
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = File::create(file)?;
    serde_json::to_writer_pretty(&mut file, &report)?;
    writeln!(file)?;
    println!("{}", json!({"blocked_targets":blocked}));
    if let Some(target) = target
        && !blocked[target].is_empty()
    {
        return Err(format!("Dependency advisories block {target}").into());
    }
    Ok(())
}
