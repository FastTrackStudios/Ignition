//! Prints the hit chart a project carries — what a person decided.
//!
//! ```text
//! cargo run -p ignition-daw --bin chart -- <project.RPP>
//! ```
//!
//! The groups are the interesting part: a group of three is a phrase,
//! and a phrase is what can be thrown left / centre / right instead of
//! flashed three times in the same place.

use anyhow::{Result, bail};

fn main() -> Result<()> {
    let path = match std::env::args().nth(1) {
        Some(p) => p,
        None => bail!("usage: chart <project.RPP>"),
    };
    let song = ignition_daw::load(&path)?;
    let chart = ignition_daw::chart::read(&path, &song)?;

    if chart.is_empty() {
        println!("no HITS track in this project");
        return Ok(());
    }

    let mut counts = std::collections::BTreeMap::new();
    for hit in &chart.hits {
        *counts.entry(hit.class.label()).or_insert(0usize) += 1;
    }
    println!(
        "{} hits, {} groups, {} ungrouped",
        chart.hits.len(),
        chart.groups.len(),
        chart.ungrouped().count()
    );
    for (label, n) in &counts {
        println!("  {label:<12} x{n}");
    }

    println!("\ngroups:");
    for (i, group) in chart.groups.iter().enumerate() {
        let members: Vec<String> = chart
            .members(group)
            .map(|h| format!("{}@{}.{:.2}", h.class.label(), h.at.bar, h.at.beat))
            .collect();
        let section = song
            .section_at(group.start)
            .map(|s| s.name.as_str())
            .unwrap_or("—");
        println!(
            "  {i:>2}  bar {:>3}.{:<4.2} {:<10} {} hits: {}",
            group.start.bar,
            group.start.beat,
            section,
            members.len(),
            members.join(", ")
        );
    }
    Ok(())
}
