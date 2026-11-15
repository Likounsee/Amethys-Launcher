use ametis_events::EventBus;
use ametis_runtime::CoreRuntime;
use ametis_storage::Database;
use std::{error::Error, path::PathBuf};

fn data_directory() -> PathBuf {
    std::env::var_os("AMETIS_DATA_DIR").map(PathBuf::from).unwrap_or_else(|| {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join(".ametis")
    })
}

fn main() -> Result<(), Box<dyn Error>> {
    ametis_logging::initialize()?;
    let data_dir = data_directory();
    std::fs::create_dir_all(&data_dir)?;
    let _database = Database::open(data_dir.join("ametis.sqlite"))?;
    let mut runtime = CoreRuntime::new(EventBus::new(128));
    runtime.start()?;
    tracing::info!("Ametis Launcher core agent initialized");
    runtime.shutdown()?;
    tracing::info!("Ametis Launcher core agent stopped");
    Ok(())
}
