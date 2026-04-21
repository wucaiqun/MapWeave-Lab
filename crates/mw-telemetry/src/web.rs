use crate::LogConfig;

pub fn init_logging(config: LogConfig) -> Result<(), log::SetLoggerError> {
    wasm_logger::init(wasm_logger::Config::new(config.level.to_filter()))
}
