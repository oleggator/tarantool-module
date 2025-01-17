use log::{warn, Level, LevelFilter};
use once_cell::sync::Lazy;
use tarantool::log::{say, SayLevel, TarantoolLogger};

/// This test panics cause logger already set in log_with_user_defined_mapping test.
pub fn zlog() {
    static TLOGGER: TarantoolLogger = TarantoolLogger::new();
    log::set_logger(&TLOGGER).unwrap();
    log::set_max_level(LevelFilter::Debug);
    warn!(target: "target", "message {}", 99);

    say(SayLevel::Warn, "<file>", 0, Some("<error>"), "<message>");
}

pub fn log_with_user_defined_mapping() {
    static TLOGGER: Lazy<TarantoolLogger> = Lazy::new(|| {
        TarantoolLogger::with_mapping(|level: Level| match level {
            Level::Warn => SayLevel::Info,
            _ => SayLevel::Warn,
        })
    });

    log::set_logger(&*TLOGGER).unwrap();
    log::set_max_level(LevelFilter::Debug);
    warn!(target: "target", "message {}", 99);

    say(SayLevel::Warn, "<file>", 0, Some("<error>"), "<message>");
}
