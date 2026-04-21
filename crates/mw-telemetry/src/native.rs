use crate::LogConfig;

pub fn init_logging(config: LogConfig) -> Result<(), log::SetLoggerError> {
    let mut builder = env_logger::Builder::new();
    builder.filter_level(config.level.to_filter());
    builder.try_init()
}
