extern crate ape;
extern crate ffav as ffmpeg;
extern crate flume;
extern crate ignore;
extern crate mime_guess;
extern crate rayon;
extern crate tree_magic;

use rayon::prelude::*;

fn main() {
    scan_library(std::env::args().nth(1).unwrap());
}

fn scan_library(path: String) {
    ffmpeg::init().unwrap();

    println!("Scanning path: {}", path);

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
            // std::thread::sleep(std::time::Duration::from_millis(10));
            println!(
                "{:?} / {:?} : {:?}",
                COUNT_COMPLETE.fetch_add(1, std::sync::atomic::Ordering::SeqCst),
                COUNT_TOTAL,
                std::thread::current().id()
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
                    println!(
                        "{:?} / {:?} : {:?}",
                        COUNT_COMPLETE,
                        COUNT_TOTAL,
                        std::thread::current().id()
                    );
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
    if let Some(mime) = mime_guess::from_path(entry.path()).first() {
        // println!("{} {}", mime, entry.path().display());
    } else {
        let mimetype = tree_magic::from_filepath(entry.path());
        // println!("{} {}", mimetype, entry.path().display());
    }

    match ffmpeg::format::input(&entry.path()) {
        Ok(context) => {
            if context.streams().best(ffmpeg::media::Type::Video).is_some()
                || context.streams().best(ffmpeg::media::Type::Audio).is_some()
            {
                for (k, v) in context.metadata().iter() {
                    // println!("        {}: {}", k.to_lowercase(), v);
                }
            }
        }

        Err(error) => {
            // println!("error: {}", error);
        }
    }

    if let Ok(tag) = ape::read(entry.path()) {
        for item in tag {
            // println!(
            //     "        [ape] {}: {:?}",
            //     item.key.to_lowercase(),
            //     item.value
            // );
        }
    }
}
