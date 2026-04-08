mod audit;
mod db;
mod mcp;
mod models;
mod prune;
mod server;
mod storage;
mod tools;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        print_usage();
        std::process::exit(1);
    }

    let command = args[1].as_str();
    match command {
        "--help" | "-h" | "help" => print_usage(),
        "--version" | "-V" | "version" => print_version(),
        "serve" => {
            if has_help_flag(&args[2..]) {
                print_serve_help();
                return;
            }
            if let Err(e) = server::run() {
                eprintln!("server error: {e}");
                std::process::exit(1);
            }
        }
        "audit" => {
            if has_help_flag(&args[2..]) {
                print_audit_help();
                return;
            }
            if let Err(e) = audit::run(&args[2..]) {
                eprintln!("audit error: {e}");
                std::process::exit(1);
            }
        }
        "prune" => {
            if has_help_flag(&args[2..]) {
                print_prune_help();
                return;
            }
            if let Err(e) = prune::run(&args[2..]) {
                eprintln!("prune error: {e}");
                std::process::exit(1);
            }
        }
        _ => {
            eprintln!("unknown command: {command}");
            eprintln!();
            print_usage();
            std::process::exit(1);
        }
    }
}

fn has_help_flag(args: &[String]) -> bool {
    args.iter().any(|a| a == "--help" || a == "-h")
}

fn print_version() {
    println!(
        "review-mcp v{} (commit: {}, built: {})",
        env!("BUILD_VERSION"),
        env!("BUILD_COMMIT"),
        env!("BUILD_DATETIME"),
    );
}

fn print_usage() {
    eprintln!("review-mcp — Deterministic review workflow MCP server");
    eprintln!();
    eprintln!("Usage:");
    eprintln!("  review-mcp <command> [options]");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  serve       Start MCP server (stdio transport)");
    eprintln!("  audit       List and inspect review sessions");
    eprintln!("  prune       Delete old sessions and review files");
    eprintln!();
    eprintln!("Flags:");
    eprintln!("  --version   Show version info");
    eprintln!("  --help      Show this help");
    eprintln!();
    eprintln!("Run 'review-mcp <command> --help' for details on each command.");
}

fn print_serve_help() {
    eprintln!("review-mcp serve — Start the MCP server");
    eprintln!();
    eprintln!("Usage:");
    eprintln!("  review-mcp serve");
    eprintln!();
    eprintln!("Starts a JSON-RPC 2.0 server on stdio (stdin/stdout).");
    eprintln!("Designed to be launched by Claude Code as an MCP server.");
    eprintln!();
    eprintln!("Configure in Claude Code:");
    eprintln!("  claude mcp add -s user review-mcp -- review-mcp serve");
    eprintln!();
    eprintln!("Tools exposed: session_create, session_get, round_start,");
    eprintln!("  review_write, review_read, round_status, round_set_outcome,");
    eprintln!("  session_signal, session_signals, session_list");
}

fn print_audit_help() {
    eprintln!("review-mcp audit — List and inspect review sessions");
    eprintln!();
    eprintln!("Usage:");
    eprintln!("  review-mcp audit                List all sessions");
    eprintln!("  review-mcp audit <id>           Show session details (partial UUID match)");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --limit <N>       Max results (default: 20)");
    eprintln!("  --offset <N>      Skip first N results (default: 0)");
    eprintln!("  --type <TYPE>     Filter by review type");
    eprintln!("                    Values: code, plan, manuscript, architecture, custom");
    eprintln!("  --status <STATUS> Filter by session status");
    eprintln!("                    Values: active, completed, abandoned");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  review-mcp audit --type code --limit 5");
    eprintln!("  review-mcp audit a1b2c3d4");
}

fn print_prune_help() {
    eprintln!("review-mcp prune — Delete old sessions and review files");
    eprintln!();
    eprintln!("Usage:");
    eprintln!("  review-mcp prune [options]");
    eprintln!();
    eprintln!("Deletes sessions older than the specified age (default: 365 days),");
    eprintln!("including all associated rounds, reviews, signals, and files on disk.");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --days <N>   Max age in days (default: 365)");
    eprintln!("  --dry-run    Show what would be pruned without deleting");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  review-mcp prune                 # Delete sessions older than 1 year");
    eprintln!("  review-mcp prune --days 90       # Delete sessions older than 90 days");
    eprintln!("  review-mcp prune --dry-run       # Preview only");
}
