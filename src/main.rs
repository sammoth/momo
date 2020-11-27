extern crate ape;
extern crate evmap;
extern crate ffav as ffmpeg;
extern crate flume;
extern crate ignore;
extern crate lazy_static;
extern crate mime_guess;
extern crate rayon;
extern crate rusqlite;
extern crate structopt;
extern crate tree_magic;

use rayon::prelude::*;
use structopt::StructOpt;

use rusqlite::{params, Connection, Result};

#[derive(StructOpt)]
#[structopt(about = "media scanner")]
enum Command {
    Scan {
        #[structopt(parse(from_os_str))]
        path: std::path::PathBuf,
    },
    // Rescan {
    //     #[structopt(parse(from_os_str))]
    //     path: std::path::PathBuf,
    // },
    Search {
        #[structopt(short)]
        field: String,
        #[structopt(short)]
        query: String,
    },
}

lazy_static::lazy_static! {
    static ref DB: std::path::PathBuf = {
    let dirs = directories_next::ProjectDirs::from("", "", "momo").unwrap();
    let db_path = dirs.cache_dir().to_path_buf();
    let mut db_file = db_path.to_path_buf();
    std::fs::create_dir_all(db_path).unwrap();
    db_file.push("library.db");
    db_file
    };
}

fn main() -> Result<()> {
    let args = Command::from_args();

    {
        let connection = Connection::open_with_flags(
            DB.as_path(),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_CREATE,
        )?;
        connection.execute("PRAGMA foreign_keys = ON;", params![])?;
        connection.execute(
            "CREATE TABLE IF NOT EXISTS metadb (
                      id              INTEGER PRIMARY KEY,
                      location        TEXT NOT NULL,
                      subsong         INTEGER
                      )",
            params![],
        )?;
        connection.execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS track ON metadb (location, subsong)",
            params![],
        )?;
        connection.execute(
            "CREATE TABLE IF NOT EXISTS tag(
                metadb              INTEGER NOT NULL,
                key                 TEXT NOT NULL, 
                value               TEXT NOT NULL,
                FOREIGN KEY(metadb) REFERENCES metadb(id),
                PRIMARY KEY (metadb, key)
              );",
            params![],
        )?;
    }

    match args {
        Command::Scan { path } => {
            scan_library(path);
        }
        // Command::Rescan { path } => {
        //     scan_library(path);
        // }
        Command::Search { field, mut query } => {
            let connection = Connection::open_with_flags(
                DB.as_path(),
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )?;

            let mut stmt = connection.prepare(
                "SELECT location, value FROM metadb m JOIN tag t on t.metadb = m.id WHERE key = ?1 and value LIKE ?2",
            )?;

            query.push('%');
            query.insert(0, '%');
            let mut rows = stmt.query(params![field, query])?;

            while let Some(row) = rows.next()? {
                let location: String = row.get(0)?;
                let value: String = row.get(1)?;
                println!("{} : {}", location, value);
            }
        }
    }

    Ok(())
}

fn scan_library(path: std::path::PathBuf) {
    ffmpeg::init().unwrap();

    println!("Scanning path: {}", path.display());

    let (tx, rx) = flume::unbounded();

    static COUNT_COMPLETE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    static COUNT_TOTAL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    let walker = ignore::WalkBuilder::new(path)
        .follow_links(true)
        .standard_filters(true)
        .hidden(true)
        .ignore(true)
        .build_parallel();

    let rayon_consumer = std::thread::spawn(move || {
        rx.into_iter()
            .par_bridge()
            .for_each(|entry| match scan_file(entry) {
                Ok(_) => {
                    println!(
                        "{:?} / {:?}",
                        COUNT_COMPLETE.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1,
                        COUNT_TOTAL,
                    );
                }
                Err(err) => {
                    println!(
                        "{:?} / {:?} (Error {:?})",
                        COUNT_COMPLETE.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1,
                        COUNT_TOTAL,
                        err,
                    );
                }
            });
    });

    walker.run(|| {
        let tx = tx.clone();
        Box::new(move |_result| {
            match _result {
                Ok(entry) => {
                    match entry.metadata() {
                        Ok(metadata) => {
                            if metadata.is_dir() {
                                return ignore::WalkState::Continue;
                            }
                        }

                        Err(_) => {
                            return ignore::WalkState::Continue;
                        }
                    }

                    COUNT_TOTAL.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    tx.send(entry).unwrap();
                }
                Err(err) => println!("ERROR: {}", err),
            }
            ignore::WalkState::Continue
        })
    });

    drop(tx);

    rayon_consumer.join().unwrap();
}

fn scan_file(entry: ignore::DirEntry) -> Result<()> {
    let path = entry.path().to_str().unwrap();

    let mime = mime_guess::from_path(entry.path())
        .first()
        .map(|mime| mime.essence_str().to_string())
        .unwrap_or(tree_magic::from_filepath(entry.path()));

    if mime.starts_with("audio") || mime.starts_with("video") {
        let connection = Connection::open_with_flags(
            DB.as_path(),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_CREATE,
        )?;
        connection.execute(
            "INSERT INTO metadb (
                location,
                subsong
            ) VALUES (?1, ?2)",
            params![path, 0],
        )?;
        let metadb_id: i32 = connection.query_row(
            "SELECT id FROM metadb WHERE location = ?1 and subsong = ?2",
            params![path, 0],
            |row| row.get(0),
        )?;

        match ffmpeg::format::input(&entry.path()) {
            Ok(context) => {
                if context.streams().best(ffmpeg::media::Type::Video).is_some()
                    || context.streams().best(ffmpeg::media::Type::Audio).is_some()
                {
                    for (k, v) in context.metadata().iter() {
                        connection.execute(
                            "INSERT INTO tag (
                                metadb,
                                key,
                                value
                            ) VALUES (?1, ?2, ?3)",
                            params![metadb_id, k.to_lowercase(), v],
                        )?;
                    }
                }
            }

            Err(error) => {
                println!(
                    "ffmpeg parsing error for {}: {}",
                    entry.path().display(),
                    error
                );
            }
        }

        if let Ok(tag) = ape::read(entry.path()) {
            for item in tag {
                if let ape::ItemValue::Text(s) = item.value {
                    connection.execute(
                        "INSERT INTO tag (
                                metadb,
                                key,
                                value
                            ) VALUES (?1, ?2, ?3)",
                        params![metadb_id, item.key.to_lowercase(), s],
                    )?;
                };
            }
        }
    }

    Ok(())
}
