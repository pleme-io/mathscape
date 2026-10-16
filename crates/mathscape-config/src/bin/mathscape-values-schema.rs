use helm_forge::values_schema::{Mode, Outcome, emit};
use mathscape_config::chart::{REGENERATE, generated_schemas};
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = Mode::from_args(&args);
    let root = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .map_or_else(|| PathBuf::from("."), PathBuf::from);
    let mut ok = true;
    for (relative, rendered) in generated_schemas() {
        let path = root.join(relative);
        match emit(&path, &rendered, mode) {
            Ok(Outcome::Fresh) => println!("{} is fresh", path.display()),
            Ok(Outcome::Written) => println!("wrote {}", path.display()),
            Ok(Outcome::Stale) => {
                eprintln!("{} is stale; regenerate it with `{REGENERATE}`", path.display());
                ok = false;
            }
            Err(e) => {
                eprintln!("{}: {e}", path.display());
                ok = false;
            }
        }
    }
    if ok { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}
