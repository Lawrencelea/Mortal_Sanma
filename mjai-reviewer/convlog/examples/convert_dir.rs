use std::env;
use std::error::Error;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::thread;

use convlog::tenhou::Log;
use convlog::tenhou_to_mjai;

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let input_dir = args
        .next()
        .expect("usage: convert_dir INPUT_DIR OUTPUT_DIR");
    let output_dir = args
        .next()
        .expect("usage: convert_dir INPUT_DIR OUTPUT_DIR");
    let input_dir = Path::new(&input_dir);
    let output_dir = Path::new(&output_dir);

    fs::create_dir_all(output_dir)?;

    let mut paths = fs::read_dir(input_dir)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    paths.retain(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"));
    paths.sort();

    let worker_count = thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1)
        .min(paths.len().max(1));
    let chunk_size = paths.len().div_ceil(worker_count);

    let (converted, failed, events) = thread::scope(|scope| {
        let handles: Vec<_> = paths
            .chunks(chunk_size)
            .map(|chunk| {
                scope.spawn(move || {
                    let mut converted = 0usize;
                    let mut failed = 0usize;
                    let mut events = 0usize;

                    for path in chunk {
                        match convert_one(path, output_dir) {
                            Ok(event_count) => {
                                converted += 1;
                                events += event_count;
                            }
                            Err(err) => {
                                failed += 1;
                                eprintln!("failed {}: {err}", path.display());
                            }
                        }
                    }

                    (converted, failed, events)
                })
            })
            .collect();

        handles
            .into_iter()
            .map(|handle| handle.join().expect("converter worker panicked"))
            .fold((0, 0, 0), |acc, item| {
                (acc.0 + item.0, acc.1 + item.1, acc.2 + item.2)
            })
    });

    writeln!(
        io::stdout(),
        "converted={converted} failed={failed} events={events}"
    )?;
    Ok(())
}

fn convert_one(path: &Path, output_dir: &Path) -> Result<usize, String> {
    let text = fs::read_to_string(path).map_err(|err| format!("read failed: {err}"))?;
    let tenhou_log = Log::from_json_str(&text).map_err(|err| format!("invalid json: {err}"))?;
    let mjai_events = tenhou_to_mjai(&tenhou_log).map_err(|err| err.to_string())?;

    let stem = path
        .file_stem()
        .ok_or_else(|| "missing file stem".to_owned())?
        .to_string_lossy();
    let out_path: PathBuf = output_dir.join(format!("{stem}.jsonl"));
    let mut out = fs::File::create(out_path).map_err(|err| format!("create failed: {err}"))?;
    for event in &mjai_events {
        serde_json::to_writer(&mut out, event).map_err(|err| format!("write failed: {err}"))?;
        out.write_all(b"\n")
            .map_err(|err| format!("write failed: {err}"))?;
    }

    Ok(mjai_events.len())
}
