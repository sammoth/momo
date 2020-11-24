#[macro_use]
extern crate diesel;
#[macro_use]
extern crate diesel_migrations;

extern crate ape;
extern crate ffav as ffmpeg;
extern crate flume;
extern crate ignore;
extern crate lazy_static;
extern crate mime_guess;
extern crate rayon;
extern crate structopt;
extern crate tree_magic;

pub mod models;
pub mod schema;

use diesel::prelude::*;
use rayon::prelude::*;
use structopt::StructOpt;

use self::models::*;

type SqlitePool = diesel::r2d2::Pool<diesel::r2d2::ConnectionManager<SqliteConnection>>;

lazy_static::lazy_static! {
    static ref POOL: SqlitePool = {
    let dirs = directories_next::ProjectDirs::from("", "", "momo").unwrap();
    let db_path = dirs.cache_dir().to_path_buf();
    let mut db_file = db_path.to_path_buf();
    std::fs::create_dir_all(db_path).unwrap();
    db_file.push("library.db");
    let db = db_file.to_str().unwrap();

    let manager = diesel::r2d2::ConnectionManager::<SqliteConnection>::new(db);
    diesel::r2d2::Pool::builder()
        .max_size(1)
        .build(manager)
        .unwrap()
    };
}

#[derive(StructOpt)]
#[structopt(about = "media scanner")]
enum Command {
    Scan {
        #[structopt(parse(from_os_str))]
        path: std::path::PathBuf,
    },
    Rescan {
        #[structopt(parse(from_os_str))]
        path: std::path::PathBuf,
    },
    // Search {
    //     #[structopt(short)]
    //     field: String,
    //     #[structopt(short)]
    //     query: String,
    // },
}

diesel_migrations::embed_migrations!();

fn main() {
    {
        let connection = POOL.get().unwrap();
        embedded_migrations::run(&connection).unwrap();
    }

    let args = Command::from_args();

    match args {
        Command::Scan { path } => {
            scan_library(path);
        }
        Command::Rescan { path } => {
            scan_library(path);
        }
    }
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
        rx.into_iter().par_bridge().for_each(|entry| {
            scan_file(entry);
            println!(
                "{:?} / {:?}",
                COUNT_COMPLETE.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1,
                COUNT_TOTAL,
            );
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

fn scan_file(entry: ignore::DirEntry) {
    let mime = mime_guess::from_path(entry.path())
        .first()
        .map(|mime| mime.essence_str().to_string())
        .unwrap_or(tree_magic::from_filepath(entry.path()));

    if mime.starts_with("audio") || mime.starts_with("video") {
        let connection = POOL.get().unwrap();

        use schema::metadbs;
        use schema::tags;

        let new_metadb = NewMetadb {
            location: entry.path().to_str().unwrap(),
            subsong: &0,
            mimetype: &mime,
        };

        diesel::insert_into(metadbs::table)
            .values(&new_metadb)
            .execute(&connection)
            .expect("error inserting");

        use schema::metadbs::dsl::*;
        let new_id: i32 = metadbs
            .order(id.desc())
            .select(id)
            .first(&connection)
            .expect("no id from insert");

        match ffmpeg::format::input(&entry.path()) {
            Ok(context) => {
                if context.streams().best(ffmpeg::media::Type::Video).is_some()
                    || context.streams().best(ffmpeg::media::Type::Audio).is_some()
                {
                    for (k, v) in context.metadata().iter() {
                        let new_tag = NewTag {
                            id: &new_id,
                            key: &k.to_lowercase(),
                            value: v,
                        };

                        diesel::insert_into(tags::table)
                            .values(&new_tag)
                            .execute(&connection);
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
                    let new_tag = NewTag {
                        id: &new_id,
                        key: &item.key.to_lowercase(),
                        value: &s,
                    };

                    diesel::insert_into(tags::table)
                        .values(&new_tag)
                        .execute(&connection);
                };
            }
        }
    }
}
