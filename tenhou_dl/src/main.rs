use anyhow::{Context, Result};
use clap::Parser;
use tenhou_dl::{
    download_json,
    filename_to_id, 
    get_filename_list, 
    get_gz_files, id_to_link, 
    parallel_extract_gz,
    process_downloads
};
use tokio::{sync::mpsc::{self},
    task,
};


#[tokio::main]
async fn main() -> Result<()> {

    let Args { format, mode, 
        input, output, 
        extract, download } = Args::parse();

    let output = output.unwrap_or_else(|| input.clone() + "_output");
    let concurent_extract = extract.unwrap_or(4);
    let concurent_download = download.unwrap_or(10);
    let format = format.unwrap_or("gz".to_string());
    let mode = mode.unwrap_or(3);

    if format == "mjlog" {
        let urls = get_filename_list(&input)
        .into_iter()
        .flat_map(|s| filename_to_id(&s))
        .map(|id| id_to_link(&id));
    // dbg!(&output);
        let client = reqwest::Client::new();
        let tasks = urls
            .map(|url| download_json(&client, url, &output))
            .collect::<Vec<_>>();
        let _ = futures::future::join_all(tasks).await;
    }

    if format == "gz" {
        let gz_files = get_gz_files(&input);

        let (sender, receiver) = mpsc::channel::<String>(1000);

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .context("Failed to build reqwest client")?;
        
        let extraction_task = task::spawn_blocking(move || {
            parallel_extract_gz(gz_files, sender, concurent_extract, mode)
        });

        let download_task = task::spawn(async move {
            process_downloads(receiver, &client, concurent_download, &output).await
        });

        let _ = extraction_task.await?;
        let _ = download_task.await?;

        println!("All done!");
    }

    Ok(())

}

#[derive(Parser)]
#[command(name = "Tenhou downloader")]
struct Args {
    #[arg(long, short)]
    format: Option<String>,
    #[arg(long, short)]
    mode: Option<u8>,
    #[arg(long, short)]
    input: String,
    #[arg(long, short)]
    output: Option<String>,
    #[arg(long, short)]
    extract: Option<usize>,
    #[arg(long, short)]
    download: Option<usize>,
}
