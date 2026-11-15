use amethys_events::EventBus;
use amethys_runtime::CoreRuntime;
use amethys_storage::Database;
use std::{error::Error, path::PathBuf};

fn data_directory() -> PathBuf {
    std::env::var_os("AMETHYS_DATA_DIR").map(PathBuf::from).unwrap_or_else(|| {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join(".amethys")
    })
}

fn main() -> Result<(), Box<dyn Error>> {
    amethys_logging::initialize()?;
    let data_dir = data_directory();
    std::fs::create_dir_all(&data_dir)?;
    let _database = Database::open(data_dir.join("amethys.sqlite"))?;
    let mut runtime = CoreRuntime::new(EventBus::new(128));
    runtime.start()?;
    tracing::info!("Amethys Launcher core agent initialized");
    runtime.shutdown()?;
    tracing::info!("Amethys Launcher core agent stopped");
    Ok(())
}
