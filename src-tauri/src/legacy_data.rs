//! One-off move of the on-device data left behind by the app's pre-rebrand
//! name, `habits`. Its bundle identifier (and so its app-data directory) was
//! `com.dcferreira.habits` and its SQLite file `habits.sqlite`; both are
//! renamed in place on first launch so existing habits, media, and logs carry
//! over to Budgebell. Purely local filesystem renames — no network I/O.

use std::io;
use std::path::Path;

/// The pre-rebrand bundle identifier, i.e. the old app-data directory name.
const LEGACY_IDENTIFIER: &str = "com.dcferreira.habits";
/// The pre-rebrand SQLite file name inside the app-data directory.
const LEGACY_DB_FILE: &str = "habits.sqlite";
/// SQLite's sidecar files that must travel with the main database file.
const DB_SUFFIXES: [&str; 4] = ["", "-wal", "-shm", "-journal"];

/// Moves `<data_root>/com.dcferreira.habits` to `<data_root>/<identifier>`
/// and renames `habits.sqlite` (plus its sidecars) to `db_file` inside it.
///
/// Never overwrites: if the new directory already exists the legacy one is
/// left untouched, and a database file is only renamed when its new name is
/// still free. A no-op on a fresh install or once migrated.
pub fn migrate(data_root: &Path, identifier: &str, db_file: &str) -> io::Result<()> {
    let legacy_dir = data_root.join(LEGACY_IDENTIFIER);
    let app_dir = data_root.join(identifier);
    if legacy_dir.is_dir() && !app_dir.exists() {
        std::fs::rename(&legacy_dir, &app_dir)?;
    }
    for suffix in DB_SUFFIXES {
        let legacy_db = app_dir.join(format!("{LEGACY_DB_FILE}{suffix}"));
        let db = app_dir.join(format!("{db_file}{suffix}"));
        if legacy_db.is_file() && !db.exists() {
            std::fs::rename(&legacy_db, &db)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const ID: &str = "com.dcferreira.budgebell";
    const DB: &str = "budgebell.sqlite";

    #[test]
    fn a_fresh_install_is_a_no_op() {
        let root = tempfile::tempdir().unwrap();
        migrate(root.path(), ID, DB).unwrap();
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }

    #[test]
    fn moves_the_legacy_directory_and_renames_the_database() {
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join(LEGACY_IDENTIFIER);
        fs::create_dir_all(legacy.join("media")).unwrap();
        fs::write(legacy.join("habits.sqlite"), "db").unwrap();
        fs::write(legacy.join("habits.sqlite-wal"), "wal").unwrap();
        fs::write(legacy.join("media").join("drill.png"), "png").unwrap();

        migrate(root.path(), ID, DB).unwrap();

        let app = root.path().join(ID);
        assert!(!legacy.exists());
        assert_eq!(fs::read_to_string(app.join(DB)).unwrap(), "db");
        assert_eq!(
            fs::read_to_string(app.join("budgebell.sqlite-wal")).unwrap(),
            "wal"
        );
        assert_eq!(
            fs::read_to_string(app.join("media").join("drill.png")).unwrap(),
            "png"
        );
        assert!(!app.join("habits.sqlite").exists());
    }

    #[test]
    fn leaves_the_legacy_directory_alone_when_the_new_one_exists() {
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join(LEGACY_IDENTIFIER);
        let app = root.path().join(ID);
        fs::create_dir_all(&legacy).unwrap();
        fs::create_dir_all(&app).unwrap();
        fs::write(legacy.join("habits.sqlite"), "old").unwrap();
        fs::write(app.join(DB), "new").unwrap();

        migrate(root.path(), ID, DB).unwrap();

        assert_eq!(
            fs::read_to_string(legacy.join("habits.sqlite")).unwrap(),
            "old"
        );
        assert_eq!(fs::read_to_string(app.join(DB)).unwrap(), "new");
    }

    #[test]
    fn renames_a_legacy_database_already_in_the_new_directory() {
        let root = tempfile::tempdir().unwrap();
        let app = root.path().join(ID);
        fs::create_dir_all(&app).unwrap();
        fs::write(app.join("habits.sqlite"), "db").unwrap();

        migrate(root.path(), ID, DB).unwrap();

        assert_eq!(fs::read_to_string(app.join(DB)).unwrap(), "db");
        assert!(!app.join("habits.sqlite").exists());
    }

    #[test]
    fn never_overwrites_an_existing_database() {
        let root = tempfile::tempdir().unwrap();
        let app = root.path().join(ID);
        fs::create_dir_all(&app).unwrap();
        fs::write(app.join("habits.sqlite"), "old").unwrap();
        fs::write(app.join(DB), "new").unwrap();

        migrate(root.path(), ID, DB).unwrap();

        assert_eq!(fs::read_to_string(app.join(DB)).unwrap(), "new");
        assert_eq!(
            fs::read_to_string(app.join("habits.sqlite")).unwrap(),
            "old"
        );
    }
}
