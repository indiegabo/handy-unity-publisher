use rusqlite::{Connection, Result};

fn main() -> Result<()> {
    let db_path = "C:/Users/indie/AppData/Local/HandyGamesPublisher/runtime/state/runtime.db";
    let conn = Connection::open_with_flags(db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;

    println!("--- Repository Info ---");
    let mut stmt = conn.prepare("SELECT id, name, enabled, polling_interval_seconds, last_seen_tag FROM repositories WHERE name = 'Revolutions'")?;
    let mut rows = stmt.query([])?;

    let id: i64 = if let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let name: String = row.get(1)?;
        let enabled: bool = row.get(2)?;
        let interval: i32 = row.get(3)?;
        let tag: Option<String> = row.get(4)?;
        println!("ID: {}, Name: {}, Enabled: {}, Polling Interval: {}s, Last Seen Tag: {:?}", id, name, enabled, interval, tag);
        id
    } else {
        println!("Repository 'Revolutions' not found.");
        return Ok(());
    };

    {
        let target_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM build_targets WHERE repository_id = ? AND enabled = 1",
            [id],
            |row| row.get(0),
        )?;
        println!("Enabled Build Target Count: {}", target_count);

        let trigger_rule_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM trigger_rules WHERE repository_id = ?",
            [id],
            |row| row.get(0),
        )?;
        println!("Trigger Rules Count: {}", trigger_rule_count);

        let poll_source_trigger_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM trigger_rules WHERE repository_id = ? AND enabled = 1 AND source = 'poll'",
            [id],
            |row| row.get(0),
        )?;
        println!("Enabled Poll-Source Trigger Rules: {}", poll_source_trigger_count);
    }

    Ok(())
}
