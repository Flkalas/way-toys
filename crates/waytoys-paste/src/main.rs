mod picker;
use std::path::PathBuf;
use waytoys_core::{Paths, client};
use waytoys_paste::{Result, service};
fn run() -> Result<()> {
    let mut daemon = false;
    let mut data = None;
    let mut status = false;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--daemon") => daemon = true,
            Some("--data-dir") => {
                data = Some(PathBuf::from(
                    args.next().ok_or("--data-dir requires a path")?,
                ))
            }
            Some("--status") => status = true,
            Some("--help" | "-h") => {
                println!(
                    "waytoys-paste [--status] | --daemon --data-dir PATH\nMeta+V: 텍스트·이미지 기록. 선택하면 복사하고 닫습니다. Ctrl+V로 붙여넣으세요."
                );
                return Ok(());
            }
            _ => return Err("usage: waytoys-paste [--status] | --daemon --data-dir PATH".into()),
        }
    }
    if daemon {
        let directory = data.ok_or("--daemon requires --data-dir")?;
        if !directory.is_absolute() {
            return Err("기록 경로는 절대 경로여야 합니다".into());
        }
        return service::run(directory);
    }
    if data.is_some() {
        return Err(
            "--data-dir is a daemon option; use waytoys --paste-data-dir for the runtime".into(),
        );
    }
    if status {
        let c = service::connection()?;
        let p = service::proxy(&c)?;
        let value: String = p.call("List", &("", "all", 0u32))?;
        println!("{value}");
        return Ok(());
    }
    if client::call("Status", "").is_err() {
        client::ensure_running(&Paths::resolve(None, None)?)?;
    }
    picker::run()
}
fn main() {
    if let Err(e) = run() {
        eprintln!("waytoys-paste: {e}");
        std::process::exit(1);
    }
}
