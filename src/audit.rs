use crate::db::Db;
use crate::models::{ReviewType, ReviewerType, SessionStatus};
use std::str::FromStr;

pub fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let db = Db::open()?;

    // Parse arguments
    if args.is_empty() {
        return list_sessions(&db, args);
    }

    let first = &args[0];

    // If it looks like a UUID (contains dashes and is longish), show detail
    if first.len() >= 8 && !first.starts_with('-') {
        return show_session_detail(&db, first);
    }

    // Otherwise treat as list with flags
    list_sessions(&db, args)
}

fn list_sessions(db: &Db, args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut limit: u32 = 20;
    let mut offset: u32 = 0;
    let mut review_type: Option<ReviewType> = None;
    let mut status: Option<SessionStatus> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--limit" => {
                i += 1;
                if i < args.len() {
                    limit = args[i].parse().unwrap_or(20);
                }
            }
            "--offset" => {
                i += 1;
                if i < args.len() {
                    offset = args[i].parse().unwrap_or(0);
                }
            }
            "--type" => {
                i += 1;
                if i < args.len() {
                    review_type = ReviewType::from_str(&args[i]).ok();
                }
            }
            "--status" => {
                i += 1;
                if i < args.len() {
                    status = SessionStatus::from_str(&args[i]).ok();
                }
            }
            _ => {}
        }
        i += 1;
    }

    let sessions = db.list_sessions(limit, offset, review_type, status)?;

    if sessions.is_empty() {
        println!("No review sessions found.");
        return Ok(());
    }

    // Header
    println!(
        "{:<38} {:<40} {:<6} {:<7} {:<10} CREATED",
        "SESSION ID", "TARGET", "TYPE", "ROUNDS", "STATUS"
    );
    println!("{}", "-".repeat(120));

    for session in &sessions {
        let round_count = db.count_rounds(&session.id).unwrap_or(0);
        let target = truncate_path(&session.target_path, 38);
        println!(
            "{:<38} {:<40} {:<6} {:<7} {:<10} {}",
            session.id,
            target,
            session.review_type,
            round_count,
            session.status,
            session.created_at
        );
    }

    Ok(())
}

fn show_session_detail(db: &Db, partial_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Try exact match first, then prefix match
    let session = match db.get_session(partial_id) {
        Ok(s) => s,
        Err(_) => {
            // Try prefix match via list
            let all = db.list_sessions(100, 0, None, None)?;
            match all.into_iter().find(|s| s.id.starts_with(partial_id)) {
                Some(s) => s,
                None => {
                    eprintln!("No session found matching: {partial_id}");
                    return Ok(());
                }
            }
        }
    };

    println!("Session: {}", session.id);
    println!("Target:  {}", session.target_path);
    println!("Type:    {}", session.review_type);
    println!("Status:  {}", session.status);
    println!("Created: {}", session.created_at);
    println!("Updated: {}", session.updated_at);

    // Rounds
    let rounds = db.get_rounds_for_session(&session.id)?;
    if !rounds.is_empty() {
        println!();
        println!("Rounds:");
        for round in &rounds {
            let outcome_str = match &round.outcome {
                Some(o) => format!("{o}"),
                None => "pending".to_string(),
            };
            println!(
                "  Round {} [{}] - {}",
                round.round_number, outcome_str, round.created_at
            );

            // Show reviews for this round
            let reviews = db.get_reviews_for_round(round.id)?;
            for reviewer in [
                ReviewerType::Regular,
                ReviewerType::Harsh,
                ReviewerType::Grounded,
            ] {
                let label = match reviewer {
                    ReviewerType::Regular => "regular: ",
                    ReviewerType::Harsh => "harsh:   ",
                    ReviewerType::Grounded => "grounded:",
                };
                match reviews.iter().find(|r| r.reviewer_type == reviewer) {
                    Some(r) => {
                        let filename = r.file_path.rsplit('/').next().unwrap_or(&r.file_path);
                        println!("    {label} {filename} ({} bytes)", r.bytes_written);
                    }
                    None => {
                        println!("    {label} (missing)");
                    }
                }
            }
        }
    }

    // Signals
    let signals = db.get_signals(&session.id)?;
    if !signals.is_empty() {
        println!();
        println!("Signals:");
        for signal in &signals {
            let comment = signal
                .comment
                .as_deref()
                .map(|c| format!(" — {c}"))
                .unwrap_or_default();
            println!(
                "  [{}] {}: {}{}",
                signal.created_at, signal.source_label, signal.signal_type, comment
            );
        }
    }

    Ok(())
}

fn truncate_path(path: &str, max_len: usize) -> String {
    if path.len() <= max_len {
        return path.to_string();
    }
    let suffix = &path[path.len() - (max_len - 3)..];
    format!("...{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_path_short() {
        assert_eq!(truncate_path("/short/path.rs", 40), "/short/path.rs");
    }

    #[test]
    fn test_truncate_path_long() {
        let long = "/very/long/path/that/exceeds/the/maximum/allowed/length/file.rs";
        let result = truncate_path(long, 30);
        assert!(result.starts_with("..."));
        assert_eq!(result.len(), 30);
    }
}
