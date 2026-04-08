use crate::db::Db;
use crate::storage;
use std::fs;

const DEFAULT_MAX_AGE_DAYS: u64 = 365;

pub fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut max_age_days = DEFAULT_MAX_AGE_DAYS;
    let mut dry_run = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--days" => {
                i += 1;
                if i < args.len() {
                    max_age_days = args[i].parse().unwrap_or(DEFAULT_MAX_AGE_DAYS);
                }
            }
            "--dry-run" => {
                dry_run = true;
            }
            _ => {}
        }
        i += 1;
    }

    let cutoff = cutoff_iso8601(max_age_days);
    let db = Db::open()?;

    if dry_run {
        let sessions = db.list_sessions(1000, 0, None, None)?;
        let stale: Vec<_> = sessions
            .iter()
            .filter(|s| s.created_at.as_str() < cutoff.as_str())
            .collect();

        if stale.is_empty() {
            println!("Nothing to prune (no sessions older than {max_age_days} days).");
            return Ok(());
        }

        println!("Dry run — would prune {} session(s):", stale.len());
        for s in &stale {
            let round_count = db.count_rounds(&s.id).unwrap_or(0);
            println!(
                "  {} ({} rounds, created {})",
                s.id, round_count, s.created_at
            );
        }

        let total_bytes = stale
            .iter()
            .map(|s| dir_size(&storage::session_dir(&s.id)))
            .sum::<u64>();
        println!(
            "Would free ~{} of review files.",
            human_bytes(total_bytes)
        );
        return Ok(());
    }

    let pruned_ids = db.prune_sessions_before(&cutoff)?;

    if pruned_ids.is_empty() {
        println!("Nothing to prune (no sessions older than {max_age_days} days).");
        return Ok(());
    }

    // Remove session directories from disk
    let mut files_freed: u64 = 0;
    let mut dirs_removed: usize = 0;
    for id in &pruned_ids {
        let dir = storage::session_dir(id);
        if dir.exists() {
            files_freed += dir_size(&dir);
            if let Err(e) = fs::remove_dir_all(&dir) {
                eprintln!("warning: failed to remove {}: {e}", dir.display());
            } else {
                dirs_removed += 1;
            }
        }
    }

    println!(
        "Pruned {} session(s), removed {} director(ies), freed {}.",
        pruned_ids.len(),
        dirs_removed,
        human_bytes(files_freed),
    );

    Ok(())
}

/// Compute the ISO 8601 timestamp for `days` ago from now.
fn cutoff_iso8601(days: u64) -> String {
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let cutoff_secs = duration.as_secs().saturating_sub(days * 86400);

    // Reuse the same epoch-to-date logic from models
    let time_of_day = cutoff_secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    let total_days = cutoff_secs / 86400;
    let (year, month, day) = days_to_ymd(total_days);

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

fn days_to_ymd(mut days: u64) -> (u64, u64, u64) {
    let mut year = 1970;
    loop {
        let diy = if is_leap_year(year) { 366 } else { 365 };
        if days < diy {
            break;
        }
        days -= diy;
        year += 1;
    }
    let month_days: [u64; 12] = if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut month = 0;
    for (i, &md) in month_days.iter().enumerate() {
        if days < md {
            month = i as u64 + 1;
            break;
        }
        days -= md;
    }
    (year, month, days + 1)
}

fn is_leap_year(year: u64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

fn dir_size(path: &std::path::Path) -> u64 {
    if !path.exists() {
        return 0;
    }
    walkdir(path)
}

fn walkdir(path: &std::path::Path) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let ft = entry.file_type().unwrap_or_else(|_| {
                // fallback: assume file
                fs::metadata(entry.path())
                    .map(|m| m.file_type())
                    .unwrap_or_else(|_| entry.file_type().unwrap())
            });
            if ft.is_dir() {
                total += walkdir(&entry.path());
            } else {
                total += entry.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    total
}

fn human_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let kb = bytes as f64 / 1024.0;
    if kb < 1024.0 {
        return format!("{kb:.1} KB");
    }
    let mb = kb / 1024.0;
    format!("{mb:.1} MB")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_human_bytes() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1024), "1.0 KB");
        assert_eq!(human_bytes(1536), "1.5 KB");
        assert_eq!(human_bytes(1048576), "1.0 MB");
    }

    #[test]
    fn test_cutoff_iso8601_format() {
        let cutoff = cutoff_iso8601(30);
        assert_eq!(cutoff.len(), 20);
        assert!(cutoff.ends_with('Z'));
        assert!(cutoff < crate::models::now_iso8601());
    }

    #[test]
    fn test_cutoff_iso8601_ordering() {
        let c365 = cutoff_iso8601(365);
        let c30 = cutoff_iso8601(30);
        // 365 days ago is before 30 days ago
        assert!(c365 < c30);
    }

    #[test]
    fn test_prune_db() {
        let db = crate::db::Db::open_memory().unwrap();

        // Create a session
        let session = db
            .create_session("/tmp/old.rs", crate::models::ReviewType::Code)
            .unwrap();
        let round = db.create_round(&session.id).unwrap();
        db.create_review(
            round.id,
            crate::models::ReviewerType::Regular,
            "/fake/path.md",
            "sha256:abc",
            100,
        )
        .unwrap();
        db.create_signal(
            &session.id,
            crate::models::SignalType::Addressed,
            "worker",
            None,
        )
        .unwrap();

        // Prune with a future cutoff — should delete everything
        let pruned = db.prune_sessions_before("2099-01-01T00:00:00Z").unwrap();
        assert_eq!(pruned.len(), 1);
        assert_eq!(pruned[0], session.id);

        // Verify everything is gone
        assert!(db.get_session(&session.id).is_err());
        assert_eq!(db.get_signals(&session.id).unwrap().len(), 0);
    }

    #[test]
    fn test_prune_db_nothing_old() {
        let db = crate::db::Db::open_memory().unwrap();
        db.create_session("/tmp/new.rs", crate::models::ReviewType::Code)
            .unwrap();

        // Prune with a past cutoff — should delete nothing
        let pruned = db.prune_sessions_before("1970-01-01T00:00:00Z").unwrap();
        assert_eq!(pruned.len(), 0);
    }
}
